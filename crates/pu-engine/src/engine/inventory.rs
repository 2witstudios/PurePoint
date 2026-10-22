use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

use pu_core::manifest;
use pu_core::protocol::*;
use pu_core::types::{AgentEntry, AgentStatus};

use super::Engine;
use super::fs_ops::is_too_broad_project_root;

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
        // `/`, `$HOME` and its ancestors are never projects, whatever an older
        // daemon recorded; drop them from the registry on disk too.
        let count = projects.len();
        let projects: Vec<String> = projects
            .into_iter()
            .filter(|root| !is_too_broad_project_root(Path::new(root), engine.home.as_deref()))
            .collect();
        if projects.len() != count
            && let Err(e) = write_registry(&path, &projects)
        {
            tracing::warn!("failed to rewrite project registry: {e}");
        }
        engine.project_registry_path = Some(path);
        *engine.registered_projects.lock().unwrap() = projects.into_iter().collect();
        // Until `start_registry_prune` has run, the project stream waits: a client
        // following it must not open a project an older client removed.
        engine.registry_ready.send_replace(false);
        Ok(engine)
    }

    /// Drops, in the background, registered folders whose manifest older clients
    /// deleted to remove the project. Reads are bounded so a stale network mount
    /// delays nothing but itself; the project stream opens once this finishes.
    /// Does nothing for an engine without a loaded registry.
    pub fn start_registry_prune(self: &std::sync::Arc<Self>) {
        if *self.registry_ready.borrow() {
            return;
        }
        let engine = std::sync::Arc::clone(self);
        tokio::spawn(async move { engine.prune_removed_projects().await });
    }

    pub(crate) async fn prune_removed_projects(&self) {
        let mut reads = tokio::task::JoinSet::new();
        for root in self.registered_projects() {
            reads.spawn(async move {
                let path = PathBuf::from(&root);
                let read = tokio::task::spawn_blocking(move || was_removed(&path));
                let removed = matches!(
                    tokio::time::timeout(Duration::from_secs(2), read).await,
                    Ok(Ok(true))
                );
                removed.then_some(root)
            });
        }
        let mut removed = Vec::new();
        while let Some(result) = reads.join_next().await {
            if let Ok(Some(root)) = result {
                removed.push(root);
            }
        }
        // The removal re-checks each manifest under the update lock; a mount that went
        // stale since must not keep the projects stream closed.
        match tokio::time::timeout(
            Duration::from_secs(5),
            self.unregister_projects(removed, Unregister::IfRemoved),
        )
        .await
        {
            Ok(Ok(_)) => {}
            Ok(Err(e)) => tracing::warn!("failed to drop removed projects from the registry: {e}"),
            Err(_) => tracing::warn!("dropping removed projects from the registry timed out"),
        }
        self.registry_ready.send_replace(true);
    }

    /// Resolves once registered projects removed by older clients are dropped.
    pub(crate) async fn registry_ready(&self) {
        let mut ready = self.registry_ready.subscribe();
        let _ = ready.wait_for(|ready| *ready).await;
    }

    /// Registers an initialized project, except one too broad to be a project
    /// (`/`, `$HOME` or an ancestor of it), which is skipped without error.
    /// Only `Init` registers: using a project never adds it back.
    pub(super) async fn register_project(&self, root: &str) -> Result<(), Response> {
        let home = self.home.clone();
        // Serialize durable updates without holding the inventory mutex over
        // filesystem I/O; health and queries can keep reading the old snapshot.
        let update = self.project_registry_updates.clone().lock_owned().await;
        let root = root.to_string();
        let projects = self.registered_projects.clone();
        let registry = self.project_registry_path.clone();
        let added = tokio::task::spawn_blocking(move || -> std::io::Result<bool> {
            // Keep serialization even if the awaiting request is cancelled.
            let _update = update;
            let root = Path::new(&root).canonicalize()?;
            if is_too_broad_project_root(&root, home.as_deref()) {
                return Ok(false);
            }
            // Registration is only valid for readable, initialized projects.
            manifest::read_manifest(&root).map_err(std::io::Error::other)?;
            let root = root.to_string_lossy().into_owned();
            let mut next: Vec<_> = {
                let current = projects
                    .lock()
                    .map_err(|_| std::io::Error::other("project registry lock poisoned"))?;
                if current.contains(&root) {
                    return Ok(false);
                }
                current.iter().cloned().collect()
            };
            next.push(root.clone());
            next.sort();
            if let Some(path) = registry {
                write_registry(&path, &next)?;
            }
            projects
                .lock()
                .map_err(|_| std::io::Error::other("project registry lock poisoned"))?
                .insert(root);
            Ok(true)
        })
        .await
        .map_err(|e| Self::inventory_error("IO_ERROR", e.to_string()))?
        .map_err(|e| Self::inventory_error("IO_ERROR", e.to_string()))?;
        if added {
            self.notify_projects_changed();
        }
        Ok(())
    }

    /// Removes registered roots and returns the ones that were registered.
    async fn unregister_projects(
        &self,
        roots: Vec<String>,
        mode: Unregister,
    ) -> std::io::Result<Vec<String>> {
        let roots: HashSet<String> = roots.into_iter().collect();
        let update = self.project_registry_updates.clone().lock_owned().await;
        let projects = self.registered_projects.clone();
        let registry = self.project_registry_path.clone();
        let removed = tokio::task::spawn_blocking(move || -> std::io::Result<Vec<String>> {
            let _update = update;
            let (removed, next): (Vec<_>, Vec<_>) = {
                let current = projects
                    .lock()
                    .map_err(|_| std::io::Error::other("project registry lock poisoned"))?;
                current.iter().cloned().partition(|root| {
                    roots.contains(root)
                        && (mode == Unregister::Always || was_removed(Path::new(root)))
                })
            };
            if removed.is_empty() {
                return Ok(removed);
            }
            let mut next = next;
            next.sort();
            if let Some(path) = registry {
                write_registry(&path, &next)?;
            }
            let mut current = projects
                .lock()
                .map_err(|_| std::io::Error::other("project registry lock poisoned"))?;
            for root in &removed {
                current.remove(root);
            }
            Ok(removed)
        })
        .await
        .map_err(std::io::Error::other)??;
        if !removed.is_empty() {
            self.notify_projects_changed();
        }
        Ok(removed)
    }

    /// Forgets a project on this daemon without touching its files. Live
    /// agents block the removal unless the caller asks to suspend them, so
    /// they can be resumed if the project is added again.
    pub(super) async fn handle_remove_project(
        &self,
        project_root: &str,
        suspend_agents: bool,
    ) -> Response {
        // A deleted folder can no longer be canonicalized; match it as given.
        let path = PathBuf::from(project_root);
        let root = tokio::task::spawn_blocking(move || path.canonicalize())
            .await
            .ok()
            .and_then(Result::ok)
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|| project_root.to_string());
        let live: Vec<String> = self
            .sessions
            .lock()
            .await
            .iter()
            .filter(|(_, h)| {
                h.project_root.as_deref() == Some(root.as_str()) && h.exit_rx.borrow().is_none()
            })
            .map(|(id, _)| id.clone())
            .collect();
        let (mut suspended, mut killed) = (Vec::new(), Vec::new());
        if !live.is_empty() {
            if !suspend_agents {
                return Self::inventory_error(
                    "PROJECT_HAS_LIVE_AGENTS",
                    format!(
                        "{root} has {} live agent{}; suspend them first or remove with suspend_agents",
                        live.len(),
                        if live.len() == 1 { "" } else { "s" }
                    ),
                );
            }
            match self.handle_suspend(&root, SuspendTarget::All).await {
                Response::SuspendResult { suspended: ids } => suspended = ids,
                // Without a manifest nothing can be suspended; its sessions are stopped below.
                Response::Error { code, .. } if code == "NOT_INITIALIZED" => {}
                // Never kill agents a readable project would have suspended.
                error => return error,
            }
            // Sessions the manifest does not list (or a missing manifest) still
            // belong to the project; nothing may keep running in a removed one.
            let rest: Vec<String> = live
                .into_iter()
                .filter(|id| !suspended.contains(id))
                .collect();
            if !rest.is_empty() {
                self.kill_agents(&rest).await;
                killed = rest;
            }
        }
        match self
            .unregister_projects(vec![root.clone()], Unregister::Always)
            .await
        {
            Ok(removed) => Response::ProjectRemoved {
                removed: !removed.is_empty(),
                project_root: root,
                suspended,
                killed,
            },
            Err(e) => Self::inventory_error("IO_ERROR", e.to_string()),
        }
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
            Request::Read { agent_id: id, .. } => Some(id.clone()),
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
        let legacy_root_use = allowed_on_too_broad_root(request);
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
            check_project_root_scope(root, legacy_root_use, self.home.as_deref())?;
            if let Some(id) = targeted.as_deref() {
                self.validate_live_owner(id, root).await?;
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
        // A global query read every registered root; a scoped one needs the registry.
        let registered: HashSet<String> = if report.project_root.is_none() {
            manifests.iter().map(|(root, _)| root.clone()).collect()
        } else {
            self.registered_projects().into_iter().collect()
        };
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
                        signal: self.signal_snapshot(&agent.id),
                        screen_aware: handle.is_some_and(|h| h.screen_aware),
                        phase: handle
                            .filter(|h| h.screen_aware)
                            .map(|h| h.output_buffer.input_phase().to_string()),
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
                    // A registered project whose manifest could not be read is
                    // missing from the report. An agent in a folder that is not
                    // a registered project counts as standalone.
                    if registered.contains(root) {
                        report.complete = false;
                    }
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
                    signal: self.signal_snapshot(id),
                    screen_aware: handle.screen_aware,
                    phase: handle
                        .screen_aware
                        .then(|| handle.output_buffer.input_phase().to_string()),
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
                signal: self.signal_snapshot(id).map(Box::new),
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

/// Requests that may still name a root too broad to be a project (`/`,
/// `$HOME` or an ancestor). An older version could record agents under
/// `$HOME` through `~/.pu/manifest.json`; these let users see, read and stop
/// them, but nothing new is created there.
fn allowed_on_too_broad_root(request: &Request) -> bool {
    matches!(
        request,
        Request::Status { .. }
            | Request::SubscribeStatus { .. }
            | Request::Read { .. }
            | Request::ChannelRead { .. }
            | Request::Kill { .. }
    )
}

#[allow(clippy::result_large_err)]
fn check_project_root_scope(
    root: &str,
    legacy_root_use: bool,
    home: Option<&Path>,
) -> Result<(), Response> {
    if legacy_root_use || !is_too_broad_project_root(Path::new(root), home) {
        return Ok(());
    }
    Err(project_root_not_allowed(root))
}

pub(super) fn project_root_not_allowed(root: &str) -> Response {
    Response::Error {
        code: "project_root_not_allowed".into(),
        message: format!(
            "{root} is /, $HOME or an ancestor of $HOME; register a project directory instead"
        ),
    }
}

/// A project folder whose manifest is gone was removed by an older client. A
/// missing folder is only unavailable: it may be on an unmounted volume.
/// Only a manifest known to be absent counts: one that cannot be checked (a
/// permission error, a stale network mount) keeps the project.
fn was_removed(root: &Path) -> bool {
    root.is_dir() && matches!(pu_core::paths::manifest_path(root).try_exists(), Ok(false))
}

/// Whether `unregister_projects` re-checks, under the update lock, that each
/// root is still a removed folder: one reopened since it was seen stays.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Unregister {
    Always,
    IfRemoved,
}

fn write_registry(path: &Path, projects: &[String]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let temporary = path.with_extension("json.tmp");
    let write = || -> std::io::Result<()> {
        use std::io::Write;
        let mut file = std::fs::File::create(&temporary)?;
        file.write_all(&serde_json::to_vec_pretty(projects)?)?;
        file.sync_all()?;
        std::fs::rename(&temporary, path)
    };
    write().inspect_err(|_| {
        let _ = std::fs::remove_file(&temporary);
    })
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
    async fn given_home_with_a_manifest_should_never_be_registered() {
        let tmp = tempfile::TempDir::new().unwrap();
        let path = tmp.path().join("registry.json");
        std::fs::create_dir_all(tmp.path().join("home/.pu")).unwrap();
        let home = tmp.path().join("home").canonicalize().unwrap();
        // `~/.pu` doubles as the global dir, so `$HOME` can look initialized.
        manifest::write_manifest(&home, &Manifest::new(home.to_string_lossy().into_owned()))
            .unwrap();
        let mut engine = Engine::with_project_registry(path.clone()).unwrap();
        engine.home = Some(home.clone());
        for root in [home.clone(), home.parent().unwrap().to_path_buf()] {
            engine
                .register_project(&root.to_string_lossy())
                .await
                .unwrap();
        }
        assert!(engine.registered_projects().is_empty());
        assert!(!path.exists());
    }

    #[tokio::test]
    async fn given_legacy_home_project_requests_should_refuse_changes_but_serve_cleanup() {
        let tmp = tempfile::TempDir::new().unwrap();
        let path = tmp.path().join("registry.json");
        std::fs::create_dir_all(tmp.path().join("home/.pu")).unwrap();
        let home = tmp.path().join("home").canonicalize().unwrap();
        let root = home.to_string_lossy().into_owned();
        // An older version recorded `$HOME` as a project in the global dir.
        manifest::write_manifest(&home, &Manifest::new(root.clone())).unwrap();
        let before = std::fs::read(pu_core::paths::manifest_path(&home)).unwrap();
        let mut engine = Engine::with_project_registry(path.clone()).unwrap();
        engine.home = Some(home.clone());

        let spawn: Request = serde_json::from_value(serde_json::json!({
            "type": "spawn", "project_root": root, "prompt": "", "agent": "terminal",
        }))
        .unwrap();
        let worktree = Request::CreateWorktree {
            project_root: root.clone(),
            name: Some("wt".into()),
            base: None,
        };
        for refused in [spawn, worktree] {
            let response = engine.handle_request(refused).await;
            assert!(
                matches!(&response, Response::Error { code, .. } if code == "project_root_not_allowed"),
                "{response:?}"
            );
        }
        assert_eq!(
            std::fs::read(pu_core::paths::manifest_path(&home)).unwrap(),
            before,
            "a refused request must not touch the legacy manifest"
        );

        let status = engine
            .handle_request(Request::Status {
                project_root: root.clone(),
                agent_id: None,
            })
            .await;
        assert!(
            matches!(status, Response::StatusReport { .. }),
            "{status:?}"
        );
        assert!(engine.registered_projects().is_empty());
        assert!(!path.exists());
    }

    #[test]
    fn given_too_broad_root_should_refuse_creating_ops_but_allow_legacy_cleanup() {
        let tmp = tempfile::TempDir::new().unwrap();
        let home = tmp.path().canonicalize().unwrap().join("home");
        std::fs::create_dir_all(&home).unwrap();
        let root = home.to_string_lossy().into_owned();
        let spawn: Request = serde_json::from_value(serde_json::json!({
            "type": "spawn", "project_root": root, "prompt": "",
        }))
        .unwrap();
        for refused in [
            spawn,
            Request::Init {
                project_root: root.clone(),
            },
            Request::CreateWorktree {
                project_root: root.clone(),
                name: None,
                base: None,
            },
        ] {
            let allowed = allowed_on_too_broad_root(&refused);
            assert!(matches!(
                check_project_root_scope(&root, allowed, Some(&home)),
                Err(Response::Error { code, .. }) if code == "project_root_not_allowed"
            ));
        }
        for cleanup in [
            Request::Status {
                project_root: root.clone(),
                agent_id: None,
            },
            Request::Kill {
                project_root: root.clone(),
                target: KillTarget::All,
                exclude: vec![],
            },
        ] {
            let allowed = allowed_on_too_broad_root(&cleanup);
            assert!(check_project_root_scope(&root, allowed, Some(&home)).is_ok());
        }
        // An ordinary project is unaffected.
        let project = home.join("code").to_string_lossy().into_owned();
        assert!(check_project_root_scope(&project, false, Some(&home)).is_ok());
    }

    #[tokio::test]
    async fn given_registry_listing_a_too_broad_root_should_purge_it_on_load() {
        let tmp = tempfile::TempDir::new().unwrap();
        let project = tmp.path().canonicalize().unwrap().join("project");
        let project = project.to_string_lossy().into_owned();
        let path = tmp.path().join("registry.json");
        std::fs::write(&path, serde_json::to_vec(&["/", project.as_str()]).unwrap()).unwrap();

        let engine = Engine::with_project_registry(path.clone()).unwrap();
        assert_eq!(engine.registered_projects(), vec![project.clone()]);
        let stored: Vec<String> = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(stored, vec![project]);
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

    #[tokio::test]
    async fn given_manifest_deleted_while_running_should_report_it_unavailable_until_the_next_prune()
     {
        let tmp = tempfile::TempDir::new().unwrap();
        let path = tmp.path().join("registry.json");
        let engine = Engine::with_project_registry(path.clone()).unwrap();
        let kept = init(&engine, &tmp.path().join("kept")).await;
        let removed = init(&engine, &tmp.path().join("removed")).await;
        // What the app does when a project is removed from the sidebar.
        std::fs::remove_file(pu_core::paths::manifest_path(Path::new(&removed))).unwrap();

        // Inventory is a read: it never changes the registry.
        let report = inventory(&engine, InventoryKind::Projects).await;
        assert!(!report.complete);
        let unavailable: Vec<_> = report.projects.iter().filter(|p| !p.available).collect();
        assert_eq!(unavailable.len(), 1);
        assert_eq!(unavailable[0].project_root, removed);
        assert_eq!(engine.registered_projects().len(), 2);
        engine.prune_removed_projects().await;
        assert_eq!(engine.registered_projects(), vec![kept.clone()]);
        drop(engine);
        assert_eq!(
            Engine::with_project_registry(path)
                .unwrap()
                .registered_projects(),
            vec![kept]
        );
    }

    #[tokio::test]
    async fn given_registry_listing_a_project_removed_by_an_older_client_should_drop_it_after_load()
    {
        let tmp = tempfile::TempDir::new().unwrap();
        let path = tmp.path().join("registry.json");
        let engine = Engine::with_project_registry(path.clone()).unwrap();
        let kept = init(&engine, &tmp.path().join("kept")).await;
        let removed = init(&engine, &tmp.path().join("removed")).await;
        // A missing folder may be an unmounted volume, so it stays.
        let unmounted = tmp.path().join("unmounted").to_string_lossy().into_owned();
        drop(engine);
        std::fs::remove_file(pu_core::paths::manifest_path(Path::new(&removed))).unwrap();
        let mut stored = vec![kept.clone(), removed, unmounted.clone()];
        stored.sort();
        std::fs::write(&path, serde_json::to_vec(&stored).unwrap()).unwrap();

        let engine = Engine::with_project_registry(path.clone()).unwrap();
        engine.prune_removed_projects().await;
        let mut expected = vec![kept, unmounted];
        expected.sort();
        assert_eq!(engine.registered_projects(), expected);
        let on_disk: Vec<String> = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(on_disk, expected);
    }

    #[tokio::test]
    async fn given_corrupt_manifest_should_keep_project_registered() {
        let tmp = tempfile::TempDir::new().unwrap();
        let engine = Engine::with_project_registry(tmp.path().join("registry.json")).unwrap();
        let root = init(&engine, &tmp.path().join("project")).await;
        std::fs::write(pu_core::paths::manifest_path(Path::new(&root)), b"corrupt").unwrap();

        let report = inventory(&engine, InventoryKind::Projects).await;
        assert!(!report.complete);
        assert_eq!(report.projects.len(), 1);
        assert!(!report.projects[0].available);
        assert_eq!(engine.registered_projects(), vec![root]);
    }

    #[tokio::test]
    async fn given_live_session_in_project_without_manifest_should_keep_it() {
        let (engine, id, tmp) = crate::test_helpers::init_and_spawn().await;
        let root = tmp.path().join("project").canonicalize().unwrap();
        std::fs::remove_file(pu_core::paths::manifest_path(&root)).unwrap();

        let report = inventory(&engine, InventoryKind::Agents).await;
        let root = root.to_string_lossy().into_owned();
        assert_eq!(report.projects.len(), 1);
        assert_eq!(report.projects[0].project_root, root);
        assert!(report.agents.iter().any(|a| a.id == id));
        assert_eq!(engine.registered_projects(), vec![root]);
        engine.kill_all_sessions(Duration::from_millis(10)).await;
    }

    #[tokio::test]
    async fn given_removed_project_reopened_should_register_again() {
        let tmp = tempfile::TempDir::new().unwrap();
        let engine = Engine::with_project_registry(tmp.path().join("registry.json")).unwrap();
        let root = init(&engine, &tmp.path().join("project")).await;
        std::fs::remove_file(pu_core::paths::manifest_path(Path::new(&root))).unwrap();
        engine.prune_removed_projects().await;
        assert!(engine.registered_projects().is_empty());

        init(&engine, Path::new(&root)).await;
        assert_eq!(engine.registered_projects(), vec![root.clone()]);
        let report = inventory(&engine, InventoryKind::Projects).await;
        assert_eq!(report.projects[0].project_root, root);
    }

    #[tokio::test]
    async fn given_removed_project_should_leave_registry_and_keep_its_files() {
        let tmp = tempfile::TempDir::new().unwrap();
        let path = tmp.path().join("registry.json");
        let engine = Engine::with_project_registry(path.clone()).unwrap();
        let root = init(&engine, &tmp.path().join("project")).await;

        let response = engine
            .handle_request(Request::RemoveProject {
                project_root: root.clone(),
                suspend_agents: false,
            })
            .await;
        assert!(
            matches!(&response, Response::ProjectRemoved { removed: true, project_root, .. } if *project_root == root),
            "{response:?}"
        );
        assert!(pu_core::paths::manifest_path(Path::new(&root)).exists());
        assert!(
            inventory(&engine, InventoryKind::Projects)
                .await
                .projects
                .is_empty()
        );
        drop(engine);
        assert!(
            Engine::with_project_registry(path)
                .unwrap()
                .registered_projects()
                .is_empty()
        );
    }

    #[tokio::test]
    async fn given_unregistered_root_should_report_nothing_removed() {
        let tmp = tempfile::TempDir::new().unwrap();
        let response = Engine::new()
            .handle_request(Request::RemoveProject {
                project_root: tmp.path().join("gone").to_string_lossy().into_owned(),
                suspend_agents: false,
            })
            .await;
        assert!(
            matches!(response, Response::ProjectRemoved { removed: false, .. }),
            "{response:?}"
        );
    }

    #[tokio::test]
    async fn given_removed_project_should_not_come_back_when_used() {
        let tmp = tempfile::TempDir::new().unwrap();
        let engine = Engine::new();
        let root = init(&engine, &tmp.path().join("project")).await;
        engine
            .handle_request(Request::RemoveProject {
                project_root: root.clone(),
                suspend_agents: false,
            })
            .await;
        // Another device still watching it must not add it back.
        engine
            .handle_request(Request::Status {
                project_root: root.clone(),
                agent_id: None,
            })
            .await;
        assert!(engine.registered_projects().is_empty());

        init(&engine, Path::new(&root)).await;
        assert_eq!(engine.registered_projects(), vec![root]);
    }

    #[tokio::test]
    async fn given_live_agents_should_refuse_removal_unless_asked_to_suspend_them() {
        let (engine, id, tmp) = crate::test_helpers::init_and_spawn().await;
        let root = tmp
            .path()
            .join("project")
            .canonicalize()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let refused = engine
            .handle_request(Request::RemoveProject {
                project_root: root.clone(),
                suspend_agents: false,
            })
            .await;
        assert!(
            matches!(&refused, Response::Error { code, .. } if code == "PROJECT_HAS_LIVE_AGENTS"),
            "{refused:?}"
        );
        assert_eq!(engine.registered_projects(), vec![root.clone()]);

        let removed = engine
            .handle_request(Request::RemoveProject {
                project_root: root.clone(),
                suspend_agents: true,
            })
            .await;
        assert!(
            matches!(&removed, Response::ProjectRemoved { removed: true, suspended, .. } if *suspended == vec![id.clone()]),
            "{removed:?}"
        );
        assert!(engine.registered_projects().is_empty());
        let agent = manifest::read_manifest(Path::new(&root)).unwrap().agents[&id].clone();
        assert!(
            agent.suspended,
            "suspended agents can be resumed once the project is added again"
        );
        engine.kill_all_sessions(Duration::from_millis(10)).await;
    }

    #[tokio::test]
    async fn given_agent_in_an_unregistered_folder_should_count_as_standalone_without_partial_inventory()
     {
        let (engine, id, tmp) = crate::test_helpers::init_and_spawn().await;
        let root = tmp
            .path()
            .join("project")
            .canonicalize()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        engine
            .unregister_projects(vec![root.clone()], Unregister::Always)
            .await
            .unwrap();
        let report = inventory(&engine, InventoryKind::Agents).await;
        assert!(
            report.complete,
            "an unregistered folder is not a missing project"
        );
        assert!(report.projects.is_empty());
        assert_eq!(report.summary.running, 1);
        assert!(report.agents.iter().any(|a| a.id == id));
        engine.kill_all_sessions(Duration::from_millis(10)).await;
    }

    #[tokio::test]
    async fn given_loaded_registry_should_hold_project_followers_until_removed_projects_are_dropped()
     {
        let tmp = tempfile::TempDir::new().unwrap();
        let engine = Engine::with_project_registry(tmp.path().join("registry.json")).unwrap();
        let wait = Duration::from_millis(50);
        assert!(
            tokio::time::timeout(wait, engine.registry_ready())
                .await
                .is_err()
        );
        engine.prune_removed_projects().await;
        assert!(
            tokio::time::timeout(wait, engine.registry_ready())
                .await
                .is_ok()
        );
        // An in-memory engine has nothing to prune.
        assert!(
            tokio::time::timeout(wait, Engine::new().registry_ready())
                .await
                .is_ok()
        );
    }

    #[tokio::test]
    async fn given_unreadable_manifest_should_refuse_removal_rather_than_kill_suspendable_agents() {
        let (engine, id, tmp) = crate::test_helpers::init_and_spawn().await;
        let root = tmp.path().join("project").canonicalize().unwrap();
        std::fs::write(pu_core::paths::manifest_path(&root), b"corrupt").unwrap();
        let root = root.to_string_lossy().into_owned();
        let response = engine
            .handle_request(Request::RemoveProject {
                project_root: root.clone(),
                suspend_agents: true,
            })
            .await;
        assert!(matches!(response, Response::Error { .. }), "{response:?}");
        assert_eq!(engine.registered_projects(), vec![root]);
        assert!(engine.sessions.lock().await.contains_key(&id));
        engine.kill_all_sessions(Duration::from_millis(10)).await;
    }

    #[tokio::test]
    async fn given_missing_manifest_should_stop_live_sessions_and_report_them_killed() {
        let (engine, id, tmp) = crate::test_helpers::init_and_spawn().await;
        let root = tmp.path().join("project").canonicalize().unwrap();
        std::fs::remove_file(pu_core::paths::manifest_path(&root)).unwrap();
        let response = engine
            .handle_request(Request::RemoveProject {
                project_root: root.to_string_lossy().into_owned(),
                suspend_agents: true,
            })
            .await;
        assert!(
            matches!(&response, Response::ProjectRemoved { removed: true, suspended, killed, .. }
                if suspended.is_empty() && *killed == vec![id.clone()]),
            "{response:?}"
        );
        engine.kill_all_sessions(Duration::from_millis(10)).await;
    }

    #[tokio::test]
    async fn given_manifest_that_cannot_be_checked_should_keep_the_project_on_prune() {
        use std::os::unix::fs::PermissionsExt;
        // Permissions do not stop root, so the stat would succeed and prove nothing.
        if unsafe { libc::geteuid() } == 0 {
            return;
        }
        let tmp = tempfile::TempDir::new().unwrap();
        let engine = Engine::with_project_registry(tmp.path().join("registry.json")).unwrap();
        let root = init(&engine, &tmp.path().join("project")).await;
        let pu = pu_core::paths::pu_dir(Path::new(&root));
        // An unreadable `.pu/` is not a removed project, whatever the manifest stat says.
        std::fs::set_permissions(&pu, std::fs::Permissions::from_mode(0o000)).unwrap();
        engine.prune_removed_projects().await;
        std::fs::set_permissions(&pu, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(engine.registered_projects(), vec![root]);
    }
}
