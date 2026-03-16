use std::collections::HashMap;

use pu_core::protocol::{AgentStatusReport, Response};
use pu_core::types::{AgentEntry, AgentStatus, WorktreeEntry, WorktreeStatus};

use crate::agent_monitor;
use crate::git;
use crate::pty_manager::AgentHandle;

use super::Engine;

impl Engine {
    pub(super) async fn handle_status(
        &self,
        project_root: &str,
        agent_id: Option<&str>,
    ) -> Response {
        // On first status call per project, reap agents whose PIDs are dead.
        // Fire-and-forget: first status returns immediately, next refresh corrects.
        let should_reap = {
            let mut reaped = self.reaped_projects.lock().unwrap();
            reaped.insert(project_root.to_string())
        }; // MutexGuard dropped here — before any .await
        if should_reap {
            let pr = project_root.to_string();
            tokio::spawn(async move {
                tokio::task::spawn_blocking(move || Self::reap_stale_agents(&pr))
                    .await
                    .ok();
            });
        }

        if let Some(id) = agent_id {
            let m = match self.read_manifest_async(project_root).await {
                Ok(m) => m,
                Err(e) => return Self::error_response(&e),
            };
            match m.find_agent(id) {
                Some(loc) => {
                    let (agent, wt_id) = match loc {
                        pu_core::types::AgentLocation::Root(a) => (a, None),
                        pu_core::types::AgentLocation::Worktree { worktree, agent } => {
                            (agent, Some(worktree.id.clone()))
                        }
                    };
                    let sessions = self.sessions.lock().await;
                    Response::AgentStatus(self.build_agent_status_report(agent, &sessions, wt_id))
                }
                None => Self::agent_not_found(id),
            }
        } else {
            match self.compute_full_status(project_root).await {
                Ok((worktrees, agents)) => Response::StatusReport { worktrees, agents },
                Err(e) => Self::error_response(&e),
            }
        }
    }

    /// Build a status report for a single agent, using live PTY state when available.
    pub(super) fn build_agent_status_report(
        &self,
        agent: &AgentEntry,
        sessions: &HashMap<String, AgentHandle>,
        worktree_id: Option<String>,
    ) -> AgentStatusReport {
        let (status, exit_code, idle_seconds) =
            self.live_agent_status_sync(&agent.id, agent, sessions);
        AgentStatusReport {
            id: agent.id.clone(),
            name: agent.name.clone(),
            agent_type: agent.agent_type.clone(),
            status,
            pid: agent.pid,
            exit_code,
            idle_seconds,
            worktree_id,
            started_at: agent.started_at,
            session_id: agent.session_id.clone(),
            prompt: agent.prompt.clone(),
            suspended: agent.suspended,
            trigger_seq_index: agent.trigger_seq_index,
            trigger_state: agent.trigger_state,
            trigger_total: agent.trigger_total,
        }
    }

    /// Compute live agent status from PTY state.
    /// Returns (status, exit_code, idle_seconds).
    pub(super) fn live_agent_status_sync(
        &self,
        id: &str,
        agent: &AgentEntry,
        sessions: &HashMap<String, AgentHandle>,
    ) -> (AgentStatus, Option<i32>, Option<u64>) {
        match sessions.get(id) {
            Some(handle) => {
                let exit_code = *handle.exit_rx.borrow();
                let status = agent_monitor::effective_status(exit_code);
                let idle_seconds = Some(handle.output_buffer.content_idle_seconds());
                (status, exit_code, idle_seconds)
            }
            // No live session — use manifest (agent already exited/killed/etc.)
            None => (agent.status, agent.exit_code, None),
        }
    }

    fn agent_pulse_entry(
        &self,
        agent: &AgentEntry,
        sessions: &HashMap<String, AgentHandle>,
        now: chrono::DateTime<chrono::Utc>,
    ) -> pu_core::protocol::AgentPulseEntry {
        let (status, exit_code, idle_seconds) =
            self.live_agent_status_sync(&agent.id, agent, sessions);
        let runtime = (now - agent.started_at).num_seconds();
        let snippet = agent.prompt.as_ref().map(|p| {
            let trimmed = p.trim();
            let truncated: String = trimmed.chars().take(77).collect();
            if truncated.len() < trimmed.len() {
                format!("{truncated}...")
            } else {
                truncated
            }
        });
        pu_core::protocol::AgentPulseEntry {
            id: agent.id.clone(),
            name: agent.name.clone(),
            agent_type: agent.agent_type.clone(),
            status,
            exit_code,
            runtime_seconds: runtime,
            idle_seconds,
            prompt_snippet: snippet,
        }
    }

    pub(super) async fn handle_pulse(&self, project_root: &str) -> Response {
        let m = match self.read_manifest_async(project_root).await {
            Ok(m) => m,
            Err(e) => return Self::error_response(&e),
        };

        let sessions = self.sessions.lock().await;
        let now = chrono::Utc::now();

        // Build root-level agents
        let root_agents: Vec<pu_core::protocol::AgentPulseEntry> = m
            .agents
            .values()
            .map(|a| self.agent_pulse_entry(a, &sessions, now))
            .collect();

        // Build worktree entries — collect all agent data in one lock acquisition
        let active_worktrees: Vec<_> = m
            .worktrees
            .values()
            .filter(|wt| wt.status == WorktreeStatus::Active)
            .cloned()
            .collect();

        let wt_agents: Vec<Vec<pu_core::protocol::AgentPulseEntry>> = active_worktrees
            .iter()
            .map(|wt| {
                wt.agents
                    .values()
                    .map(|a| self.agent_pulse_entry(a, &sessions, now))
                    .collect()
            })
            .collect();

        // Drop sessions lock before shelling out to git
        drop(sessions);

        let mut worktrees = Vec::new();
        for (wt, agents) in active_worktrees.iter().zip(wt_agents) {
            let elapsed = (now - wt.created_at).num_seconds();

            // Get git diff stats
            let wt_path = std::path::PathBuf::from(&wt.path);
            let (files_changed, insertions, deletions, diff_error) = if wt_path.exists() {
                let base = wt.base_branch.as_deref();
                match git::diff_worktree(&wt_path, base, true).await {
                    Ok(output) => (
                        output.files_changed,
                        output.insertions,
                        output.deletions,
                        None,
                    ),
                    Err(e) => (0, 0, 0, Some(format!("{e}"))),
                }
            } else {
                (0, 0, 0, None)
            };

            worktrees.push(pu_core::protocol::WorktreePulseEntry {
                worktree_id: wt.id.clone(),
                worktree_name: wt.name.clone(),
                branch: wt.branch.clone(),
                elapsed_seconds: elapsed,
                agents,
                files_changed,
                insertions,
                deletions,
                diff_error,
            });
        }

        Response::PulseReport {
            worktrees,
            root_agents,
        }
    }

    pub(super) async fn handle_diff(
        &self,
        project_root: &str,
        worktree_id: Option<&str>,
        stat: bool,
    ) -> Response {
        let m = match self.read_manifest_async(project_root).await {
            Ok(m) => m,
            Err(e) => return Self::error_response(&e),
        };

        let worktrees: Vec<WorktreeEntry> = if let Some(wt_id) = worktree_id {
            match m.worktrees.get(wt_id) {
                Some(wt) => vec![wt.clone()],
                None => {
                    return Response::Error {
                        code: "NOT_FOUND".into(),
                        message: format!("worktree '{wt_id}' not found"),
                    };
                }
            }
        } else {
            m.worktrees
                .into_values()
                .filter(|wt| wt.status == WorktreeStatus::Active)
                .collect()
        };

        if worktrees.is_empty() {
            return Response::DiffResult { diffs: vec![] };
        }

        let is_targeted = worktree_id.is_some();
        let mut diffs = Vec::new();
        for wt in &worktrees {
            let wt_path = std::path::PathBuf::from(&wt.path);
            if !wt_path.exists() {
                if is_targeted {
                    // Targeted query: report the error so callers can distinguish
                    // a deleted worktree from a clean one.
                    diffs.push(pu_core::protocol::WorktreeDiffEntry {
                        worktree_id: wt.id.clone(),
                        worktree_name: wt.name.clone(),
                        branch: wt.branch.clone(),
                        base_branch: wt.base_branch.clone(),
                        diff_output: String::new(),
                        files_changed: 0,
                        insertions: 0,
                        deletions: 0,
                        error: Some(format!("worktree directory not found: {}", wt.path)),
                    });
                }
                // Bulk query: skip missing dirs (best-effort)
                continue;
            }
            let base = wt.base_branch.as_deref();
            match git::diff_worktree(&wt_path, base, stat).await {
                Ok(output) => {
                    diffs.push(pu_core::protocol::WorktreeDiffEntry {
                        worktree_id: wt.id.clone(),
                        worktree_name: wt.name.clone(),
                        branch: wt.branch.clone(),
                        base_branch: wt.base_branch.clone(),
                        diff_output: output.diff,
                        files_changed: output.files_changed,
                        insertions: output.insertions,
                        deletions: output.deletions,
                        error: None,
                    });
                }
                Err(e) => {
                    tracing::warn!("failed to diff worktree {}: {}", wt.id, e);
                    diffs.push(pu_core::protocol::WorktreeDiffEntry {
                        worktree_id: wt.id.clone(),
                        worktree_name: wt.name.clone(),
                        branch: wt.branch.clone(),
                        base_branch: wt.base_branch.clone(),
                        diff_output: String::new(),
                        files_changed: 0,
                        insertions: 0,
                        deletions: 0,
                        error: Some(format!("{e}")),
                    });
                }
            }
        }

        Response::DiffResult { diffs }
    }
}
