use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

use pu_core::manifest;
use pu_core::protocol::*;
use pu_core::types::{AgentEntry, AgentStatus};

use super::Engine;

impl Engine {
    /// Production daemons persist inventory beside their socket. `new()` remains
    /// an isolated, in-memory engine for embedded use and tests.
    pub fn with_project_registry(path: PathBuf) -> std::io::Result<Self> {
        let projects: Vec<String> = match std::fs::read(&path) {
            Ok(data) => serde_json::from_slice(&data)?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(e) => return Err(e),
        };
        if projects.iter().any(|root| !Path::new(root).is_absolute()) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "project registry roots must be absolute",
            ));
        }
        let mut engine = Self::new();
        engine.project_registry_path = Some(path);
        *engine.registered_projects.lock().unwrap() = projects.into_iter().collect();
        Ok(engine)
    }

    pub(super) async fn register_project(&self, root: &str) -> Result<(), Response> {
        // Serialize durable updates without holding the inventory mutex over
        // filesystem I/O; health and queries can keep reading the old snapshot.
        let update = self.project_registry_updates.clone().lock_owned().await;
        let root = root.to_string();
        let projects = self.registered_projects.clone();
        let registry = self.project_registry_path.clone();
        tokio::task::spawn_blocking(move || -> std::io::Result<()> {
            // Keep serialization even if the awaiting request is cancelled.
            let _update = update;
            let root = Path::new(&root).canonicalize()?;
            // Registration is only valid for readable, initialized projects.
            manifest::read_manifest(&root).map_err(std::io::Error::other)?;
            let root = root.to_string_lossy().into_owned();
            let mut next: Vec<_> = {
                let current = projects
                    .lock()
                    .map_err(|_| std::io::Error::other("project registry lock poisoned"))?;
                if current.contains(&root) {
                    return Ok(());
                }
                current.iter().cloned().collect()
            };
            next.push(root.clone());
            next.sort();
            if let Some(path) = registry {
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                let temporary = path.with_extension("json.tmp");
                let write = || -> std::io::Result<()> {
                    use std::io::Write;
                    let mut file = std::fs::File::create(&temporary)?;
                    file.write_all(&serde_json::to_vec_pretty(&next)?)?;
                    file.sync_all()?;
                    std::fs::rename(&temporary, &path)
                };
                write().inspect_err(|_| {
                    let _ = std::fs::remove_file(&temporary);
                })?;
            }
            projects
                .lock()
                .map_err(|_| std::io::Error::other("project registry lock poisoned"))?
                .insert(root);
            Ok(())
        })
        .await
        .map_err(|e| Self::inventory_error("IO_ERROR", e.to_string()))?
        .map_err(|e| Self::inventory_error("IO_ERROR", e.to_string()))
    }

    pub(super) fn inventory_error(code: &str, message: String) -> Response {
        Response::Error {
            code: code.into(),
            message,
        }
    }

    /// Empty scope on a targeted legacy operation asks the daemon to resolve
    /// ownership. Bulk operations always require a project.
    pub(crate) async fn prepare_request(&self, request: &mut Request) -> Result<(), Response> {
        let targeted = match request {
            Request::Status {
                agent_id: Some(id), ..
            }
            | Request::Resume { agent_id: id, .. }
            | Request::Rename { agent_id: id, .. }
            | Request::AssignTrigger { agent_id: id, .. }
            | Request::Kill {
                target: KillTarget::Agent(id),
                ..
            }
            | Request::Suspend {
                target: SuspendTarget::Agent(id),
                ..
            } => Some(id.clone()),
            _ => None,
        };
        let is_init = matches!(request, Request::Init { .. });
        if let Some(root) = request.project_root_mut() {
            if root.is_empty() {
                let Some(id) = targeted.as_deref() else {
                    return Err(Self::inventory_error(
                        "PROJECT_REQUIRED",
                        "bulk and project operations require a project root".into(),
                    ));
                };
                *root = self.resolve_agent(id, None).await?.0.unwrap_or_default();
                // Standalone shells have no manifest; dispatch their supported
                // targeted operations directly against the owned session.
                if root.is_empty() {
                    return Ok(());
                }
            }
            let path = PathBuf::from(&*root);
            if let Ok(Ok(canonical)) =
                tokio::task::spawn_blocking(move || path.canonicalize()).await
            {
                *root = canonical.to_string_lossy().into_owned();
            }
            if let Some(id) = targeted.as_deref() {
                self.validate_live_owner(id, root).await?;
            }
            // Init may create the manifest; every other successful project use
            // registers an already-initialized project without inventing one.
            if !is_init && self.read_manifest_async(root).await.is_ok() {
                self.register_project(root).await?;
            }
        }
        Ok(())
    }

    pub(super) async fn inventory(
        &self,
        project_root: Option<String>,
        kind: InventoryKind,
        filter: Option<AgentQueryState>,
    ) -> InventoryReport {
        let project_root = match project_root {
            Some(root) => {
                let path = PathBuf::from(&root);
                Some(
                    tokio::task::spawn_blocking(move || path.canonicalize())
                        .await
                        .ok()
                        .and_then(Result::ok)
                        .map(|p| p.to_string_lossy().into_owned())
                        .unwrap_or(root),
                )
            }
            None => None,
        };
        let roots = project_root
            .clone()
            .map(|r| vec![r])
            .unwrap_or_else(|| self.registered_projects());
        let mut report = InventoryReport {
            observed_at: chrono::Utc::now(),
            project_root,
            complete: true,
            summary: InventoryCounts::default(),
            projects: Vec::new(),
            agents: Vec::new(),
            worktrees: Vec::new(),
        };
        // Read independently so one slow project cannot block the whole query.
        let mut reads = tokio::task::JoinSet::new();
        for root in roots {
            reads.spawn(async move {
                let path = PathBuf::from(&root);
                let read = tokio::task::spawn_blocking(move || manifest::read_manifest(&path));
                let result = match tokio::time::timeout(Duration::from_secs(2), read).await {
                    Ok(Ok(result)) => result.map_err(|e| e.to_string()),
                    Ok(Err(e)) => Err(e.to_string()),
                    Err(_) => Err("project manifest read timed out".into()),
                };
                (root, result)
            });
        }
        let mut manifests = Vec::new();
        while let Some(result) = reads.join_next().await {
            match result {
                Ok(entry) => manifests.push(entry),
                Err(_) => report.complete = false,
            }
        }
        manifests.sort_by(|a, b| a.0.cmp(&b.0));
        let sessions = self.sessions.lock().await;
        let mut observed_sessions = HashSet::new();
        for (root, result) in manifests {
            let mut project = ProjectInventoryEntry {
                name: Path::new(&root)
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned(),
                project_root: root.clone(),
                available: result.is_ok(),
                error: None,
                counts: InventoryCounts {
                    projects: 1,
                    ..Default::default()
                },
            };
            let m = match result {
                Ok(m) => m,
                Err(error) => {
                    report.complete = false;
                    project.error = Some(error);
                    report.projects.push(project);
                    continue;
                }
            };
            let mut add_agent = |agent: &AgentEntry, worktree_id: Option<String>, cwd: &str| {
                let handle = sessions
                    .get(&agent.id)
                    .filter(|h| h.project_root.as_deref() == Some(root.as_str()));
                if handle.is_some() {
                    observed_sessions.insert(agent.id.clone());
                }
                let exit_code = handle
                    .map(|h| *h.exit_rx.borrow())
                    .unwrap_or(agent.exit_code);
                let state = if handle.is_some() {
                    if exit_code.is_none() {
                        AgentQueryState::Running
                    } else {
                        AgentQueryState::Broken
                    }
                } else if agent.status == AgentStatus::Broken {
                    AgentQueryState::Broken
                } else if agent.suspended {
                    AgentQueryState::Suspended
                } else {
                    AgentQueryState::Unknown
                };
                project.counts.agents += 1;
                match state {
                    AgentQueryState::Running => {
                        project.counts.running += 1;
                        if agent.agent_type == "terminal" {
                            project.counts.running_terminals += 1;
                        } else {
                            project.counts.running_ai_agents += 1;
                        }
                    }
                    AgentQueryState::Suspended => project.counts.suspended += 1,
                    AgentQueryState::Broken => project.counts.broken += 1,
                    AgentQueryState::Unknown => project.counts.unknown += 1,
                }
                if kind == InventoryKind::Agents && filter.is_none_or(|f| f == state) {
                    report.agents.push(AgentInventoryEntry {
                        project_root: Some(root.clone()),
                        cwd: cwd.to_string(),
                        worktree_id,
                        id: agent.id.clone(),
                        name: agent.name.clone(),
                        agent_type: agent.agent_type.clone(),
                        state,
                        pid: handle.map(|h| h.pid),
                        exit_code,
                        idle_seconds: handle.map(|h| h.output_buffer.content_idle_seconds()),
                        started_at: agent.started_at,
                    });
                }
            };
            for agent in m.agents.values() {
                add_agent(agent, None, &root);
            }
            for wt in m.worktrees.values() {
                for agent in wt.agents.values() {
                    add_agent(agent, Some(wt.id.clone()), &wt.path);
                }
                if kind == InventoryKind::Worktrees {
                    let mut agent_ids: Vec<_> = wt.agents.keys().cloned().collect();
                    agent_ids.sort();
                    report.worktrees.push(WorktreeInventoryEntry {
                        project_root: root.clone(),
                        id: wt.id.clone(),
                        name: wt.name.clone(),
                        path: wt.path.clone(),
                        branch: wt.branch.clone(),
                        status: wt.status,
                        agent_ids,
                    });
                }
            }
            project.counts.worktrees = m.worktrees.len();
            report.projects.push(project);
        }
        // Live ownership is authoritative even while a project manifest is
        // unavailable, or between process creation and the manifest write.
        let mut standalone = InventoryCounts::default();
        for (id, handle) in sessions.iter().filter(|(id, h)| {
            !observed_sessions.contains(*id)
                && report
                    .project_root
                    .as_ref()
                    .is_none_or(|r| h.project_root.as_ref() == Some(r))
        }) {
            let exit_code = *handle.exit_rx.borrow();
            let state = if exit_code.is_none() {
                AgentQueryState::Running
            } else {
                AgentQueryState::Broken
            };
            let counts = if let Some(root) = &handle.project_root {
                if let Some(project) = report.projects.iter_mut().find(|p| &p.project_root == root)
                {
                    &mut project.counts
                } else {
                    report.complete = false;
                    &mut standalone
                }
            } else {
                &mut standalone
            };
            counts.agents += 1;
            if state == AgentQueryState::Running {
                counts.running += 1;
                if handle.agent_type == "terminal" {
                    counts.running_terminals += 1;
                } else {
                    counts.running_ai_agents += 1;
                }
            } else {
                counts.broken += 1;
            }
            if kind == InventoryKind::Agents && filter.is_none_or(|f| f == state) {
                report.agents.push(AgentInventoryEntry {
                    project_root: handle.project_root.clone(),
                    cwd: handle.cwd.clone(),
                    worktree_id: handle.worktree_id.clone(),
                    id: id.clone(),
                    name: handle.name.clone(),
                    agent_type: handle.agent_type.clone(),
                    state,
                    pid: Some(handle.pid),
                    exit_code,
                    idle_seconds: Some(handle.output_buffer.content_idle_seconds()),
                    started_at: handle.started_at,
                });
            }
        }
        drop(sessions);
        report.summary = standalone;
        report.summary.projects = report.projects.len();
        for p in &report.projects {
            report.summary.worktrees += p.counts.worktrees;
            report.summary.agents += p.counts.agents;
            report.summary.running += p.counts.running;
            report.summary.suspended += p.counts.suspended;
            report.summary.broken += p.counts.broken;
            report.summary.unknown += p.counts.unknown;
            report.summary.running_ai_agents += p.counts.running_ai_agents;
            report.summary.running_terminals += p.counts.running_terminals;
        }
        report
            .agents
            .sort_by(|a, b| (&a.project_root, &a.id).cmp(&(&b.project_root, &b.id)));
        report
            .worktrees
            .sort_by(|a, b| (&a.project_root, &a.id).cmp(&(&b.project_root, &b.id)));
        report
    }

    pub(super) async fn handle_standalone_request(&self, request: &Request) -> Option<Response> {
        let (root, id) = match request {
            Request::Status {
                project_root,
                agent_id: Some(id),
            }
            | Request::Kill {
                project_root,
                target: KillTarget::Agent(id),
                ..
            }
            | Request::Suspend {
                project_root,
                target: SuspendTarget::Agent(id),
            }
            | Request::Resume {
                project_root,
                agent_id: id,
            }
            | Request::Rename {
                project_root,
                agent_id: id,
                ..
            }
            | Request::AssignTrigger {
                project_root,
                agent_id: id,
                ..
            } => (project_root, id),
            _ => return None,
        };
        if !root.is_empty() {
            return None;
        }
        let sessions = self.sessions.lock().await;
        let handle = sessions.get(id)?;
        if handle.project_root.is_some() {
            return None;
        }
        if matches!(request, Request::Status { .. }) {
            let exit_code = *handle.exit_rx.borrow();
            return Some(Response::AgentStatus(AgentStatusReport {
                id: id.clone(),
                name: "shell".into(),
                agent_type: "terminal".into(),
                status: crate::agent_monitor::effective_status(exit_code),
                pid: Some(handle.pid),
                exit_code,
                idle_seconds: Some(handle.output_buffer.content_idle_seconds()),
                worktree_id: None,
                started_at: handle.started_at,
                session_id: None,
                prompt: None,
                suspended: false,
                trigger_seq_index: None,
                trigger_state: None,
                trigger_total: None,
            }));
        }
        drop(sessions);
        if let Request::Kill { exclude, .. } = request {
            if exclude.contains(id) {
                return Some(Response::KillResult {
                    killed: Vec::new(),
                    exit_codes: Default::default(),
                    skipped: vec![id.clone()],
                });
            }
            let handles = self.kill_agents(std::slice::from_ref(id)).await;
            let exit_codes = handles
                .iter()
                .map(|(id, h)| (id.clone(), *h.exit_rx.borrow()))
                .collect();
            return Some(Response::KillResult {
                killed: handles.into_iter().map(|(id, _)| id).collect(),
                exit_codes,
                skipped: Vec::new(),
            });
        }
        Some(Self::inventory_error("UNSUPPORTED_AGENT_OPERATION", "standalone shells support status, logs, send, attach and kill; resumable operations require a project agent".into()))
    }

    async fn validate_live_owner(&self, id: &str, root: &str) -> Result<(), Response> {
        if let Some(handle) = self.sessions.lock().await.get(id)
            && handle.project_root.as_deref() != Some(root)
        {
            return Err(Self::inventory_error(
                "AGENT_OWNERSHIP_CONFLICT",
                format!("agent '{id}' has a live session owned by another scope"),
            ));
        }
        Ok(())
    }

    pub(super) async fn resolve_agent(
        &self,
        id: &str,
        project_root: Option<String>,
    ) -> Result<(Option<String>, Option<String>), Response> {
        let report = self
            .inventory(project_root, InventoryKind::Agents, None)
            .await;
        let matches: Vec<_> = report.agents.iter().filter(|a| a.id == id).collect();
        if matches.len() > 1 {
            return Err(Self::inventory_error(
                "AMBIGUOUS_AGENT",
                format!("agent '{id}' exists in multiple projects; specify a project"),
            ));
        }
        // The daemon's live ownership index can still route a known process
        // when another project's durable inventory is unreadable.
        if let Some(a) = matches.first() {
            let sessions = self.sessions.lock().await;
            if sessions
                .get(id)
                .is_some_and(|h| h.project_root == a.project_root && h.exit_rx.borrow().is_none())
            {
                return Ok((a.project_root.clone(), a.worktree_id.clone()));
            }
        }
        if !report.complete {
            return Err(Self::inventory_error("INVENTORY_INCOMPLETE", "cannot resolve ownership while a project is unavailable; specify a readable project".into()));
        }
        match matches.first() {
            Some(a) => {
                if let Some(root) = &a.project_root {
                    self.validate_live_owner(id, root).await?;
                }
                Ok((a.project_root.clone(), a.worktree_id.clone()))
            }
            None => Err(Self::agent_not_found(id)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pu_core::types::{Manifest, WorktreeEntry, WorktreeStatus};

    async fn init(engine: &Engine, root: &Path) -> String {
        std::fs::create_dir_all(root).unwrap();
        let root = root.canonicalize().unwrap().to_string_lossy().into_owned();
        assert!(matches!(
            engine
                .handle_request(Request::Init {
                    project_root: root.clone()
                })
                .await,
            Response::InitResult { .. }
        ));
        root
    }

    async fn inventory(engine: &Engine, kind: InventoryKind) -> InventoryReport {
        match engine
            .handle_request(Request::Inventory {
                project_root: None,
                kind,
                state: None,
            })
            .await
        {
            Response::Inventory(report) => report,
            other => panic!("unexpected response: {other:?}"),
        }
    }

    #[tokio::test]
    async fn given_concurrent_registration_should_persist_every_project() {
        let tmp = tempfile::TempDir::new().unwrap();
        let path = tmp.path().join("registry.json");
        let engine = Engine::with_project_registry(path.clone()).unwrap();
        let first = tmp.path().join("first");
        let second = tmp.path().join("second");
        let (first, second) = tokio::join!(init(&engine, &first), init(&engine, &second));
        assert_eq!(
            Engine::with_project_registry(path)
                .unwrap()
                .registered_projects(),
            vec![first, second]
        );
    }

    #[tokio::test]
    async fn given_registry_write_failure_should_report_failure_and_not_claim_registration() {
        let tmp = tempfile::TempDir::new().unwrap();
        let parent = tmp.path().join("blocked");
        let engine = Engine::with_project_registry(parent.join("registry.json")).unwrap();
        std::fs::write(&parent, b"not a directory").unwrap();
        let response = engine
            .handle_request(Request::Init {
                project_root: tmp.path().join("project").to_string_lossy().into_owned(),
            })
            .await;
        assert!(matches!(response, Response::Error { code, .. } if code == "IO_ERROR"));
        assert!(engine.registered_projects().is_empty());
    }

    #[tokio::test]
    async fn given_unreadable_live_project_should_still_include_the_owned_agent() {
        let (engine, id, tmp) = crate::test_helpers::init_and_spawn().await;
        std::fs::write(tmp.path().join("project/.pu/manifest.json"), b"corrupt").unwrap();
        let report = inventory(&engine, InventoryKind::Agents).await;
        assert!(!report.complete);
        assert_eq!(report.summary.running, 1);
        assert_eq!(report.agents[0].id, id);
        assert!(report.agents[0].project_root.is_some());
        assert!(report.projects[0].error.is_some());
        assert!(engine.resolve_agent(&id, None).await.is_ok());
        engine.kill_all_sessions(Duration::from_millis(10)).await;
    }

    #[tokio::test]
    async fn given_registered_projects_should_survive_restart_and_deduplicate_aliases() {
        let tmp = tempfile::TempDir::new().unwrap();
        let path = tmp.path().join("registry.json");
        let engine = Engine::with_project_registry(path.clone()).unwrap();
        let root = init(&engine, &tmp.path().join("project")).await;
        let alias = tmp.path().join("alias");
        std::os::unix::fs::symlink(&root, &alias).unwrap();
        engine
            .handle_request(Request::Status {
                project_root: alias.to_string_lossy().into_owned(),
                agent_id: None,
            })
            .await;
        assert_eq!(engine.registered_projects(), vec![root.clone()]);
        drop(engine);
        let engine = Engine::with_project_registry(path).unwrap();
        assert_eq!(engine.registered_projects(), vec![root]);
        assert_eq!(
            inventory(&engine, InventoryKind::Projects)
                .await
                .summary
                .projects,
            1
        );
        assert_eq!(engine.sessions.lock().await.len(), 0);
    }

    #[tokio::test]
    async fn given_invalid_project_should_not_pollute_the_registry() {
        let tmp = tempfile::TempDir::new().unwrap();
        let engine = Engine::new();
        let response = engine
            .handle_request(Request::Status {
                project_root: tmp.path().to_string_lossy().into_owned(),
                agent_id: None,
            })
            .await;
        assert!(matches!(response, Response::Error { .. }));
        assert!(engine.registered_projects().is_empty());
    }

    #[tokio::test]
    async fn given_corrupt_registry_should_report_an_error_instead_of_an_empty_inventory() {
        let tmp = tempfile::TempDir::new().unwrap();
        let path = tmp.path().join("registry.json");
        std::fs::write(&path, b"corrupted").unwrap();
        assert!(Engine::with_project_registry(path).is_err());
    }

    #[tokio::test]
    async fn given_multiple_projects_should_flatten_agents_and_worktrees_with_consistent_counts() {
        let (engine, live_id, tmp) = crate::test_helpers::init_and_spawn().await;
        let first = tmp.path().join("project").canonicalize().unwrap();
        let second = init(&engine, &tmp.path().join("second")).await;
        let source = manifest::read_manifest(&first).unwrap().agents[&live_id].clone();
        let mut m = manifest::read_manifest(Path::new(&second)).unwrap();
        let mut suspended = source.clone();
        suspended.id = "ag-suspended".into();
        suspended.agent_type = "codex".into();
        suspended.suspended = true;
        suspended.pid = None;
        suspended.prompt = Some("a large private prompt".repeat(100));
        m.agents.insert(suspended.id.clone(), suspended);
        let mut unknown = source.clone();
        unknown.id = "ag-unknown".into();
        m.agents.insert(unknown.id.clone(), unknown);
        let mut broken = source;
        broken.id = "ag-broken".into();
        broken.status = AgentStatus::Broken;
        broken.exit_code = Some(0);
        let mut agents = indexmap::IndexMap::new();
        agents.insert(broken.id.clone(), broken);
        m.worktrees.insert(
            "wt-1".into(),
            WorktreeEntry {
                id: "wt-1".into(),
                name: "feature".into(),
                path: format!("{second}/.pu/worktrees/wt-1"),
                branch: "pu/feature".into(),
                base_branch: None,
                status: WorktreeStatus::Active,
                agents,
                created_at: chrono::Utc::now(),
                merged_at: None,
                error: None,
            },
        );
        manifest::write_manifest(Path::new(&second), &m).unwrap();
        let report = inventory(&engine, InventoryKind::Agents).await;
        assert!(report.complete);
        assert_eq!(report.summary.projects, 2);
        assert_eq!(report.summary.agents, 4);
        assert_eq!(report.summary.running, 1);
        assert_eq!(report.summary.running_terminals, 1);
        assert_eq!(report.summary.running_ai_agents, 0);
        assert_eq!(report.summary.suspended, 1);
        assert_eq!(report.summary.broken, 1);
        assert_eq!(report.summary.unknown, 1);
        assert_eq!(
            report
                .agents
                .iter()
                .filter(|a| a.state == AgentQueryState::Running)
                .count(),
            report.summary.running
        );
        assert!(
            !serde_json::to_string(&report)
                .unwrap()
                .contains("private prompt")
        );
        let filtered = engine
            .inventory(None, InventoryKind::Agents, Some(AgentQueryState::Running))
            .await;
        assert_eq!(filtered.agents.len(), 1);
        assert_eq!(filtered.agents[0].id, live_id);
        let worktrees = inventory(&engine, InventoryKind::Worktrees).await;
        assert_eq!(worktrees.worktrees.len(), 1);
        assert_eq!(worktrees.worktrees[0].project_root, second);
        assert_eq!(worktrees.worktrees[0].agent_ids, vec!["ag-broken"]);
        let summary = inventory(&engine, InventoryKind::Summary).await;
        assert_eq!(summary.summary.agents, 4);
        assert!(summary.agents.is_empty());
        assert!(summary.worktrees.is_empty());
        engine.kill_all_sessions(Duration::from_millis(10)).await;
    }

    #[tokio::test]
    async fn given_agent_id_without_scope_should_route_status_and_kill_to_its_project() {
        let (engine, id, tmp) = crate::test_helpers::init_and_spawn().await;
        init(&engine, &tmp.path().join("other")).await;
        let status = engine
            .handle_request(Request::Status {
                project_root: String::new(),
                agent_id: Some(id.clone()),
            })
            .await;
        assert!(matches!(status, Response::AgentStatus(a) if a.id == id));
        let kill = engine
            .handle_request(Request::Kill {
                project_root: String::new(),
                target: KillTarget::Agent(id.clone()),
                exclude: Vec::new(),
            })
            .await;
        assert!(matches!(kill, Response::KillResult { killed, .. } if killed == vec![id.clone()]));
        assert!(!engine.sessions.lock().await.contains_key(&id));
    }

    #[tokio::test]
    async fn given_unavailable_project_should_report_partial_inventory_and_require_scope_for_resolution()
     {
        let (engine, id, tmp) = crate::test_helpers::init_and_spawn().await;
        let missing = init(&engine, &tmp.path().join("missing")).await;
        std::fs::remove_dir_all(&missing).unwrap();
        let report = inventory(&engine, InventoryKind::Agents).await;
        assert!(!report.complete);
        assert_eq!(report.projects.len(), 2);
        assert!(
            report
                .projects
                .iter()
                .any(|p| !p.available && p.error.is_some())
        );
        assert!(engine.resolve_agent(&id, None).await.is_ok());
        assert!(matches!(engine.resolve_agent("ag-unobserved", None).await,
            Err(Response::Error { code, .. }) if code == "INVENTORY_INCOMPLETE"));
        let root = tmp.path().join("project").to_string_lossy().into_owned();
        assert!(engine.resolve_agent(&id, Some(root)).await.is_ok());
        engine.kill_all_sessions(Duration::from_millis(10)).await;
    }

    #[tokio::test]
    async fn given_duplicate_agent_ids_should_require_scope_and_count_only_the_owned_process() {
        let (engine, id, tmp) = crate::test_helpers::init_and_spawn().await;
        let first = tmp.path().join("project");
        let second = init(&engine, &tmp.path().join("duplicate")).await;
        let source = manifest::read_manifest(&first).unwrap().agents[&id].clone();
        let mut m = Manifest::new(second.clone());
        m.agents.insert(id.clone(), source);
        manifest::write_manifest(Path::new(&second), &m).unwrap();
        assert!(
            matches!(engine.resolve_agent(&id, None).await, Err(Response::Error { code, .. }) if code == "AMBIGUOUS_AGENT")
        );
        assert_eq!(
            inventory(&engine, InventoryKind::Agents)
                .await
                .summary
                .running,
            1
        );
        assert!(
            matches!(engine.resolve_agent(&id, Some(second.clone())).await,
            Err(Response::Error { code, .. }) if code == "AGENT_OWNERSHIP_CONFLICT")
        );
        let result = engine
            .handle_request(Request::Kill {
                project_root: second,
                target: KillTarget::Agent(id.clone()),
                exclude: Vec::new(),
            })
            .await;
        assert!(
            matches!(result, Response::Error { code, .. } if code == "AGENT_OWNERSHIP_CONFLICT")
        );
        assert!(engine.sessions.lock().await.contains_key(&id));
        engine.kill_all_sessions(Duration::from_millis(10)).await;
    }

    #[tokio::test]
    async fn given_exited_session_before_reaping_should_not_count_as_running() {
        let (engine, id, _tmp) = crate::test_helpers::init_and_spawn().await;
        let handle = engine.sessions.lock().await;
        let mut exit = handle[&id].exit_rx.clone();
        engine
            .pty_host
            .write_to_fd(&handle[&id].master_fd(), b"exit\r")
            .await
            .unwrap();
        drop(handle);
        tokio::time::timeout(Duration::from_secs(5), async {
            while exit.borrow_and_update().is_none() {
                exit.changed().await.unwrap();
            }
        })
        .await
        .unwrap();
        assert!(engine.sessions.lock().await.contains_key(&id));
        assert_eq!(
            inventory(&engine, InventoryKind::Agents)
                .await
                .summary
                .running,
            0
        );
        assert!(matches!(
            engine.handle_request(Request::Health).await,
            Response::HealthReport { agent_count: 0, .. }
        ));
    }
}
