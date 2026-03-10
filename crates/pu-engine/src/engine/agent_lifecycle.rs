use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

use pu_core::config;
use pu_core::error::PuError;
use pu_core::manifest;
use pu_core::protocol::{KillTarget, Response, SuspendTarget};
use pu_core::types::AgentStatus;

use crate::pty_manager::{AgentHandle, SpawnConfig};

use super::Engine;

impl Engine {
    pub(super) async fn handle_kill(
        &self,
        project_root: &str,
        target: KillTarget,
        exclude: &[String],
    ) -> Response {
        let m = match self.read_manifest_async(project_root).await {
            Ok(m) => m,
            Err(e) => return Self::error_response(&e),
        };

        let all_ids: Vec<String> = match &target {
            KillTarget::Agent(id) => vec![id.clone()],
            KillTarget::Worktree(wt_id) => match m.worktrees.get(wt_id) {
                Some(wt) => wt.agents.keys().cloned().collect(),
                None => {
                    return Response::Error {
                        code: "WORKTREE_NOT_FOUND".into(),
                        message: format!("worktree {wt_id} not found"),
                    };
                }
            },
            KillTarget::All => {
                let mut ids: Vec<String> = m.agents.keys().cloned().collect();
                for wt in m.worktrees.values() {
                    ids.extend(wt.agents.keys().cloned());
                }
                ids
            }
            KillTarget::AllWorktrees => {
                let mut ids: Vec<String> = Vec::new();
                for wt in m.worktrees.values() {
                    ids.extend(wt.agents.keys().cloned());
                }
                ids
            }
        };

        // Apply exclusions (self-protection + root-protection)
        let (agent_ids, skipped): (Vec<String>, Vec<String>) =
            all_ids.into_iter().partition(|id| !exclude.contains(id));

        // Kill agents: remove pending inputs, extract handles, kill PTY processes.
        let handles_killed = self.kill_agents(&agent_ids).await;
        let exit_codes: HashMap<String, Option<i32>> = handles_killed
            .iter()
            .map(|(id, handle)| (id.clone(), *handle.exit_rx.borrow()))
            .collect();

        // Update manifest: remove all targeted agents (off async runtime)
        let killed = agent_ids.clone();
        let pr = project_root.to_string();
        let killed_for_manifest = killed.clone();
        tokio::task::spawn_blocking(move || {
            manifest::update_manifest(Path::new(&pr), move |mut m| {
                for id in &killed_for_manifest {
                    m.agents.shift_remove(id);
                    for wt in m.worktrees.values_mut() {
                        wt.agents.shift_remove(id);
                    }
                }
                m
            })
            .ok();
        })
        .await
        .ok();

        self.notify_status_change(project_root).await;

        Response::KillResult {
            killed,
            exit_codes,
            skipped,
        }
    }

    pub(super) async fn handle_rename(
        &self,
        project_root: &str,
        agent_id: &str,
        name: &str,
    ) -> Response {
        let pr = project_root.to_string();
        let aid = agent_id.to_string();
        let new_name = name.to_string();
        let new_name2 = new_name.clone();

        let result = tokio::task::spawn_blocking(move || {
            manifest::update_manifest(Path::new(&pr), |mut m| {
                if let Some(agent) = m.find_agent_mut(&aid) {
                    agent.name = new_name.clone();
                }
                m
            })
        })
        .await;

        match result {
            Ok(Ok(updated)) => {
                let found = updated.find_agent(agent_id).is_some();
                if found {
                    self.notify_status_change(project_root).await;
                    Response::RenameResult {
                        agent_id: agent_id.to_string(),
                        name: new_name2,
                    }
                } else {
                    Self::agent_not_found(agent_id)
                }
            }
            Ok(Err(e)) => Self::error_response(&e),
            Err(e) => Response::Error {
                code: "INTERNAL_ERROR".into(),
                message: format!("rename task failed: {e}"),
            },
        }
    }

    pub(super) async fn handle_assign_trigger(
        &self,
        project_root: &str,
        agent_id: &str,
        trigger_name: &str,
    ) -> Response {
        let pr = project_root.to_string();
        let tn = trigger_name.to_string();

        let trigger = tokio::task::spawn_blocking(move || {
            pu_core::trigger_def::triggers_for_event(
                Path::new(&pr),
                &pu_core::trigger_def::TriggerEvent::AgentIdle,
            )
            .into_iter()
            .find(|t| t.name == tn)
        })
        .await;

        let trigger = match trigger {
            Ok(Some(t)) => t,
            Ok(None) => {
                return Response::Error {
                    code: "NOT_FOUND".into(),
                    message: format!("trigger '{trigger_name}' not found"),
                };
            }
            Err(e) => {
                return Response::Error {
                    code: "INTERNAL_ERROR".into(),
                    message: format!("trigger lookup failed: {e}"),
                };
            }
        };

        let sequence_len = trigger.sequence.len() as u32;
        if sequence_len == 0 {
            return Response::Error {
                code: "INVALID_TRIGGER".into(),
                message: format!("trigger '{trigger_name}' has empty sequence"),
            };
        }

        // Verify the agent exists in the manifest before assigning
        let pr_check = project_root.to_string();
        let aid_check = agent_id.to_string();
        let agent_exists = tokio::task::spawn_blocking(move || {
            manifest::read_manifest(Path::new(&pr_check))
                .map(|m| m.find_agent(&aid_check).is_some())
        })
        .await;

        match agent_exists {
            Ok(Ok(false)) | Ok(Err(_)) => {
                return Response::Error {
                    code: "NOT_FOUND".into(),
                    message: format!("agent '{agent_id}' not found"),
                };
            }
            Err(e) => {
                return Response::Error {
                    code: "INTERNAL_ERROR".into(),
                    message: format!("agent lookup failed: {e}"),
                };
            }
            Ok(Ok(true)) => {} // proceed
        }

        let pr2 = project_root.to_string();
        let aid2 = agent_id.to_string();
        let tn2 = trigger_name.to_string();
        let result = tokio::task::spawn_blocking(move || {
            manifest::update_manifest(Path::new(&pr2), |mut m| {
                if let Some(agent) = m.find_agent_mut(&aid2) {
                    agent.trigger_name = Some(tn2.clone());
                    agent.trigger_state = Some(pu_core::types::TriggerState::Active);
                    agent.trigger_seq_index = Some(0);
                    agent.trigger_total = Some(sequence_len);
                    agent.gate_attempts = Some(0);
                }
                m
            })
        })
        .await;

        match result {
            Ok(Ok(_)) => {
                self.notify_status_change(project_root).await;
                Response::AssignTriggerResult {
                    agent_id: agent_id.to_string(),
                    trigger_name: trigger_name.to_string(),
                    sequence_len,
                }
            }
            Ok(Err(e)) => Self::error_response(&e),
            Err(e) => Response::Error {
                code: "INTERNAL_ERROR".into(),
                message: format!("assign trigger task failed: {e}"),
            },
        }
    }

    pub(super) async fn handle_suspend(
        &self,
        project_root: &str,
        target: SuspendTarget,
    ) -> Response {
        let m = match self.read_manifest_async(project_root).await {
            Ok(m) => m,
            Err(e) => return Self::error_response(&e),
        };

        // Collect suspendable agents — must be alive and not already suspended.
        let agent_ids: Vec<String> = match &target {
            SuspendTarget::Agent(id) => match m.find_agent(id) {
                Some(loc) => {
                    let agent = match loc {
                        pu_core::types::AgentLocation::Root(a) => a,
                        pu_core::types::AgentLocation::Worktree { agent, .. } => agent,
                    };
                    if !agent.status.is_alive() || agent.suspended {
                        return Response::SuspendResult { suspended: vec![] };
                    }
                    vec![id.clone()]
                }
                None => return Self::agent_not_found(id),
            },
            SuspendTarget::All => m
                .all_agents()
                .into_iter()
                .filter(|a| a.status.is_alive() && !a.suspended)
                .map(|a| a.id.clone())
                .collect(),
        };

        if agent_ids.is_empty() {
            return Response::SuspendResult { suspended: vec![] };
        }

        self.kill_agents(&agent_ids).await;

        // Update manifest: mark as suspended, clear pid, set suspended_at.
        // Status stays as-is (Waiting); suspended flag is metadata.
        let suspended = agent_ids.clone();
        let pr = project_root.to_string();
        let suspended_for_manifest = suspended.clone();
        tokio::task::spawn_blocking(move || {
            manifest::update_manifest(Path::new(&pr), move |mut m| {
                let now = chrono::Utc::now();
                for id in &suspended_for_manifest {
                    if let Some(agent) = m.find_agent_mut(id) {
                        agent.status = AgentStatus::Waiting;
                        agent.suspended = true;
                        agent.pid = None;
                        agent.suspended_at = Some(now);
                    }
                }
                m
            })
            .ok();
        })
        .await
        .ok();

        self.notify_status_change(project_root).await;

        Response::SuspendResult { suspended }
    }

    pub(super) async fn handle_resume(&self, project_root: &str, agent_id: &str) -> Response {
        let root_path = Path::new(project_root);

        // 1. Read manifest, find the suspended agent
        let m = match self.read_manifest_async(project_root).await {
            Ok(m) => m,
            Err(e) => return Self::error_response(&e),
        };

        let (agent_entry, _worktree_id, cwd) = match m.find_agent(agent_id) {
            Some(pu_core::types::AgentLocation::Root(a)) => {
                (a.clone(), None::<String>, project_root.to_string())
            }
            Some(pu_core::types::AgentLocation::Worktree { worktree, agent }) => (
                agent.clone(),
                Some(worktree.id.clone()),
                worktree.path.clone(),
            ),
            None => return Self::agent_not_found(agent_id),
        };

        if !agent_entry.suspended {
            return Response::Error {
                code: "INVALID_STATE".into(),
                message: "agent is not suspended".into(),
            };
        }

        // 2. Load agent config
        let cfg = match config::load_config_strict(root_path) {
            Ok(c) => c,
            Err(e) => {
                return Response::Error {
                    code: "CONFIG_ERROR".into(),
                    message: format!("failed to load config: {e}"),
                };
            }
        };
        let agent_cfg = match config::resolve_agent(&cfg, &agent_entry.agent_type) {
            Some(c) => c.clone(),
            None => {
                return Response::Error {
                    code: "INVALID_ARGUMENT".into(),
                    message: format!("unknown agent type: {}", agent_entry.agent_type),
                };
            }
        };

        // 3. Repair corrupted session files before resume (claude-code#24304)
        if let Some(ref sid) = agent_entry.session_id {
            let cwd_clone = cwd.clone();
            let sid_clone = sid.clone();
            tokio::task::spawn_blocking(move || {
                Self::repair_session_files(&cwd_clone, &sid_clone);
            })
            .await
            .ok();
        }
        let effective_session_id = agent_entry.session_id.clone();

        // 4. Construct resume command based on agent type
        let (command, args, session_id) = match self.build_resume_command(
            &agent_entry.agent_type,
            &agent_cfg,
            effective_session_id.as_deref(),
        ) {
            Ok(result) => result,
            Err(response) => return response,
        };

        // 5. Spawn PTY process
        let mut env = self.agent_env().await;
        env.push(("PU_AGENT_ID".into(), agent_id.to_string()));
        let spawn_config = SpawnConfig {
            command,
            args,
            cwd,
            env,
            env_remove: vec!["CLAUDECODE".into()],
            cols: 120,
            rows: 40,
        };

        let handle = match self.pty_host.spawn(spawn_config).await {
            Ok(h) => h,
            Err(e) => {
                return Response::Error {
                    code: "RESUME_FAILED".into(),
                    message: format!("failed to spawn process: {e}"),
                };
            }
        };

        let pid = handle.pid;

        // Store handle in session map BEFORE writing manifest.
        // ManifestWatcher in Swift fires on manifest write and immediately
        // tries to attach — the session must already be in the map.
        self.sessions
            .lock()
            .await
            .insert(agent_id.to_string(), handle);

        // 6. Update manifest: Suspended → Running, new PID
        let aid = agent_id.to_string();
        let sid = session_id.clone();
        let pr = project_root.to_string();
        let manifest_result = tokio::task::spawn_blocking(move || {
            manifest::update_manifest(Path::new(&pr), move |mut m| {
                if let Some(agent) = m.find_agent_mut(&aid) {
                    agent.status = AgentStatus::Streaming;
                    agent.suspended = false;
                    agent.pid = Some(pid);
                    agent.completed_at = None;
                    agent.suspended_at = None;
                    if let Some(ref s) = sid {
                        agent.session_id = Some(s.clone());
                    }
                }
                m
            })
        })
        .await
        .unwrap_or_else(|e| Err(PuError::Io(std::io::Error::other(e))));

        if let Err(e) = manifest_result {
            // Rollback: remove session and kill process
            if let Some(handle) = self.sessions.lock().await.remove(agent_id) {
                self.pty_host
                    .kill(&handle, Duration::from_secs(2))
                    .await
                    .ok();
            }
            return Response::Error {
                code: "RESUME_FAILED".into(),
                message: format!("failed to update manifest: {e}"),
            };
        }

        self.notify_status_change(project_root).await;

        Response::ResumeResult {
            agent_id: agent_id.to_string(),
            status: AgentStatus::Streaming,
        }
    }

    /// Construct the resume command for a given agent type.
    /// Returns Ok((command, args, session_id)) or Err(Response) on failure.
    #[allow(clippy::result_large_err)]
    pub(super) fn build_resume_command(
        &self,
        agent_type: &str,
        agent_cfg: &pu_core::types::AgentConfig,
        session_id: Option<&str>,
    ) -> Result<(String, Vec<String>, Option<String>), Response> {
        let launch_args =
            pu_core::types::resolved_launch_args(agent_type, agent_cfg.launch_args.as_deref());
        match agent_type {
            "claude" => {
                let sid = session_id.ok_or_else(|| Response::Error {
                    code: "RESUME_FAILED".into(),
                    message: "cannot resume Claude agent: no session_id preserved".into(),
                })?;
                let mut args = launch_args;
                args.push("--resume".into());
                args.push(sid.to_string());
                Ok(("claude".into(), args, Some(sid.to_string())))
            }
            "codex" => {
                // Top-level flags (e.g. --full-auto) must precede the subcommand
                let mut args = launch_args;
                args.push("resume".into());
                args.push("--last".into());
                Ok(("codex".into(), args, None))
            }
            "opencode" => {
                let mut args = vec!["--continue".into()];
                args.extend(launch_args);
                Ok(("opencode".into(), args, None))
            }
            _ => {
                // Terminal / unknown: fresh shell in same directory
                let (command, args) = Self::parse_agent_command(agent_cfg, agent_type)?;
                Ok((command, args, None))
            }
        }
    }

    /// Remove pending inputs and session handles for the given agent IDs, then kill their
    /// PTY processes. Returns the extracted handles (for callers that need exit codes).
    pub(super) async fn kill_agents(&self, agent_ids: &[String]) -> Vec<(String, AgentHandle)> {
        {
            let mut pending_inputs = self.pending_initial_inputs.lock().await;
            for id in agent_ids {
                pending_inputs.remove(id);
            }
        }
        let handles: Vec<(String, AgentHandle)> = {
            let mut sessions = self.sessions.lock().await;
            agent_ids
                .iter()
                .filter_map(|id| sessions.remove(id).map(|h| (id.clone(), h)))
                .collect()
        };
        for (id, handle) in &handles {
            if let Err(e) = self.pty_host.kill(handle, Duration::from_secs(5)).await {
                tracing::debug!(agent_id = id, "kill failed: {e}");
            }
        }
        handles
    }
}
