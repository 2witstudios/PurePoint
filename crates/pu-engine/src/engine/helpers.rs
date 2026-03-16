use std::path::Path;

use pu_core::error::PuError;
use pu_core::manifest;
use pu_core::protocol::Response;
use pu_core::types::{AgentEntry, AgentStatus, Manifest};

use crate::daemon_lifecycle;

use super::Engine;

impl Engine {
    /// Parse an agent config's command string into (program, args), resolving
    /// the "shell" sentinel to the user's login shell.
    #[allow(clippy::result_large_err)]
    pub(super) fn parse_agent_command(
        agent_cfg: &pu_core::types::AgentConfig,
        agent_type: &str,
    ) -> Result<(String, Vec<String>), Response> {
        let mut parts: Vec<String> = agent_cfg
            .command
            .split_whitespace()
            .map(String::from)
            .collect();
        if parts.is_empty() {
            return Err(Response::Error {
                code: "CONFIG_ERROR".into(),
                message: format!("agent type '{agent_type}' has an empty command"),
            });
        }
        let command = parts.remove(0);
        let command = if command == "shell" {
            std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into())
        } else {
            command
        };
        Ok((command, parts))
    }

    pub(super) fn should_inject_prompt_via_stdin(
        agent_type: &str,
        interactive: bool,
        prompt: &str,
    ) -> bool {
        !prompt.is_empty() && interactive && matches!(agent_type, "claude" | "terminal")
    }

    pub(super) fn resolved_prompt_flag(
        agent_type: &str,
        prompt_flag: Option<&str>,
    ) -> Option<String> {
        match (agent_type, prompt_flag) {
            ("opencode", None) => Some("--prompt".to_string()),
            (_, Some(flag)) => Some(flag.to_string()),
            _ => None,
        }
    }

    /// On daemon restart, reconcile agents that appear alive in the manifest but have no
    /// live process. Resumable agents (claude, codex, opencode) with a session_id get marked
    /// suspended so the Swift side can auto-resume them. Others get marked Broken.
    /// Called synchronously inside handle_init so state is correct before the first status read.
    pub(super) fn reconcile_agents_on_init(project_root: &str) {
        let root = Path::new(project_root);
        let Ok(m) = manifest::read_manifest(root) else {
            return;
        };
        let is_stale = |a: &AgentEntry| !a.suspended && matches!(a.status, AgentStatus::Running);
        let has_stale = m
            .agents
            .values()
            .chain(m.worktrees.values().flat_map(|wt| wt.agents.values()))
            .any(is_stale);
        if !has_stale {
            return;
        }
        let is_resumable = |t: &str| matches!(t, "claude" | "codex" | "opencode");
        let now = chrono::Utc::now();
        manifest::update_manifest(root, move |mut m| {
            for agent in m.agents.values_mut().chain(
                m.worktrees
                    .values_mut()
                    .flat_map(|wt| wt.agents.values_mut()),
            ) {
                if !agent.suspended && matches!(agent.status, AgentStatus::Running) {
                    if agent.session_id.is_some() && is_resumable(&agent.agent_type) {
                        agent.status = AgentStatus::Running;
                        agent.suspended = true;
                        agent.pid = None;
                        agent.suspended_at = Some(now);
                    } else {
                        agent.status = AgentStatus::Broken;
                        agent.completed_at = Some(now);
                    }
                }
            }
            m
        })
        .ok();
    }

    /// Scan the manifest for Running/Idle agents whose PID is dead, mark them Lost.
    /// Called once per project on the first status request after daemon (re)start.
    /// Note: Suspended agents are intentionally unaffected — they have no PID and are paused.
    pub(super) fn reap_stale_agents(project_root: &str) {
        let root = Path::new(project_root);
        let Ok(m) = manifest::read_manifest(root) else {
            return;
        };
        let needs_reap = |a: &AgentEntry| {
            !a.suspended
                && matches!(a.status, AgentStatus::Running)
                && a.pid
                    .is_none_or(|pid| !daemon_lifecycle::is_process_alive(pid))
        };
        let has_stale = m
            .agents
            .values()
            .chain(m.worktrees.values().flat_map(|wt| wt.agents.values()))
            .any(needs_reap);
        if !has_stale {
            return;
        }
        manifest::update_manifest(root, move |mut m| {
            let now = chrono::Utc::now();
            for agent in m.agents.values_mut().chain(
                m.worktrees
                    .values_mut()
                    .flat_map(|wt| wt.agents.values_mut()),
            ) {
                if !agent.suspended
                    && matches!(agent.status, AgentStatus::Running)
                    && agent
                        .pid
                        .is_none_or(|pid| !daemon_lifecycle::is_process_alive(pid))
                {
                    agent.status = AgentStatus::Broken;
                    agent.completed_at = Some(now);
                }
            }
            m
        })
        .ok();
    }

    pub(super) fn agent_not_found(agent_id: &str) -> Response {
        Response::Error {
            code: "AGENT_NOT_FOUND".into(),
            message: format!("no active session for agent {agent_id}"),
        }
    }

    pub(super) fn error_response(e: &PuError) -> Response {
        Response::Error {
            code: e.code().into(),
            message: e.to_string(),
        }
    }

    /// Read manifest from disk (off async runtime).
    pub(super) async fn read_manifest_async(
        &self,
        project_root: &str,
    ) -> Result<Manifest, PuError> {
        let pr = project_root.to_string();
        tokio::task::spawn_blocking(move || manifest::read_manifest(Path::new(&pr)))
            .await
            .unwrap_or_else(|e| Err(PuError::Io(std::io::Error::other(e))))
    }

    // --- Scope resolution helper ---

    pub(super) fn resolve_scope_dir(
        project_root: &str,
        scope: &str,
        local_fn: fn(&Path) -> std::path::PathBuf,
        global_fn: fn() -> Result<std::path::PathBuf, std::io::Error>,
    ) -> Result<std::path::PathBuf, String> {
        match scope {
            "global" => global_fn().map_err(|e| e.to_string()),
            "local" => Ok(local_fn(Path::new(project_root))),
            other => Err(format!(
                "unknown scope: {other} (expected 'local' or 'global')"
            )),
        }
    }
}
