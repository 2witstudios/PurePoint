use std::path::Path;

use pu_core::error::PuError;
use pu_core::manifest;
use pu_core::protocol::Response;
use pu_core::types::{AgentEntry, AgentStatus, Manifest};

use crate::daemon_lifecycle;

use super::Engine;

/// Agent types that can continue their conversation with a stored session id.
fn is_resumable(a: &AgentEntry) -> bool {
    a.session_id.is_some() && matches!(a.agent_type.as_str(), "claude" | "codex" | "opencode")
}

/// Retire an agent the manifest says is running but no daemon is running: a
/// resumable agent becomes suspended so it can be resumed, anything else Broken.
fn retire_stale_agent(agent: &mut AgentEntry, now: chrono::DateTime<chrono::Utc>) {
    if is_resumable(agent) {
        agent.suspended = true;
        agent.pid = None;
        agent.suspended_at = Some(now);
    } else {
        agent.status = AgentStatus::Broken;
        agent.completed_at = Some(now);
    }
}

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

    /// Reconcile agents that the manifest says are running but this daemon is not
    /// running (`is_live` says whether it is). Liveness is asked at the point of
    /// use, not snapshotted, because an agent can be spawned or resumed while
    /// reconcile runs (stopping orphans can take seconds).
    ///
    /// Resumable agents (claude, codex, opencode) with a session_id get marked
    /// suspended so the Swift side can auto-resume them. Others get marked Broken.
    ///
    /// A resumable agent whose process outlived its daemon is still writing its
    /// transcript; it is stopped first, because resuming next to it would put two
    /// writers on one conversation.
    /// Called synchronously inside handle_init so state is correct before the first status read.
    pub(super) fn reconcile_agents_on_init(project_root: &str, is_live: impl Fn(&str) -> bool) {
        let root = Path::new(project_root);
        let Ok(m) = manifest::read_manifest(root) else {
            return;
        };
        let is_stale = |a: &AgentEntry| {
            !a.suspended && matches!(a.status, AgentStatus::Running) && !is_live(&a.id)
        };
        let stale: Vec<&AgentEntry> = m
            .agents
            .values()
            .chain(m.worktrees.values().flat_map(|wt| wt.agents.values()))
            .filter(|a| is_stale(a))
            .collect();
        if stale.is_empty() {
            return;
        }
        for a in stale.iter().filter(|a| is_resumable(a)) {
            if is_live(&a.id) {
                continue;
            }
            if let (Some(pid), Some(sid)) = (a.pid, a.session_id.as_deref())
                && Self::is_orphaned_session(pid, sid)
            {
                tracing::warn!(agent_id = %a.id, pid, "stopping orphaned agent before resume");
                Self::stop_process_group(pid);
            }
        }
        let now = chrono::Utc::now();
        manifest::update_manifest(root, move |mut m| {
            for agent in m.agents.values_mut().chain(
                m.worktrees
                    .values_mut()
                    .flat_map(|wt| wt.agents.values_mut()),
            ) {
                if is_stale(agent) {
                    retire_stale_agent(agent, now);
                }
            }
            m
        })
        .ok();
    }

    /// True if `pid` is alive and is still the agent process for `session_id`
    /// (not an unrelated process that reused the pid).
    fn is_orphaned_session(pid: u32, session_id: &str) -> bool {
        if !daemon_lifecycle::is_process_alive(pid) {
            return false;
        }
        std::process::Command::new("ps")
            .args(["-o", "command=", "-p", &pid.to_string()])
            .stdin(std::process::Stdio::null())
            .output()
            .is_ok_and(|out| String::from_utf8_lossy(&out.stdout).contains(session_id))
    }

    /// SIGTERM the process group led by `pid` (agents are spawned with setsid),
    /// wait up to 3s, then SIGKILL.
    fn stop_process_group(pid: u32) {
        let Ok(pgid) = i32::try_from(pid) else {
            return;
        };
        unsafe {
            libc::killpg(pgid, libc::SIGTERM);
        }
        for _ in 0..30 {
            if !daemon_lifecycle::is_process_alive(pid) {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        unsafe {
            libc::killpg(pgid, libc::SIGKILL);
        }
    }

    /// Scan the manifest for Running agents whose PID is dead and retire them
    /// (resumable ones become suspended, the rest Broken; see `retire_stale_agent`).
    /// Called once per project on the first status request after daemon (re)start,
    /// which can come before any init (e.g. the app restarting the daemon and
    /// refreshing), so it must leave stopped agents just as resumable as init does.
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
                    retire_stale_agent(agent, now);
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

#[cfg(test)]
mod reconcile_tests {
    use super::*;
    use tempfile::TempDir;

    fn agent(id: &str, agent_type: &str, session_id: Option<&str>) -> AgentEntry {
        serde_json::from_value(serde_json::json!({
            "id": id,
            "name": id,
            "agentType": agent_type,
            "status": "running",
            "startedAt": "2026-01-01T00:00:00Z",
            "pid": 999_999_999u32,
            "sessionId": session_id,
            "suspended": false,
        }))
        .unwrap()
    }

    fn project_with(entries: Vec<AgentEntry>) -> TempDir {
        let tmp = TempDir::new().unwrap();
        let manifest_path = pu_core::paths::manifest_path(tmp.path());
        std::fs::create_dir_all(manifest_path.parent().unwrap()).unwrap();
        let mut m = Manifest::new(tmp.path().to_string_lossy().into_owned());
        for e in entries {
            m.agents.insert(e.id.clone(), e);
        }
        manifest::write_manifest(tmp.path(), &m).unwrap();
        tmp
    }

    fn read(tmp: &TempDir, id: &str) -> AgentEntry {
        manifest::read_manifest(tmp.path()).unwrap().agents[id].clone()
    }

    fn root(tmp: &TempDir) -> String {
        tmp.path().to_string_lossy().into_owned()
    }

    #[test]
    fn given_agent_live_in_this_daemon_should_leave_it_running() {
        let tmp = project_with(vec![agent("ag-1", "claude", Some("sid-1"))]);
        Engine::reconcile_agents_on_init(&root(&tmp), |id| id == "ag-1");

        let a = read(&tmp, "ag-1");
        assert!(!a.suspended);
        assert_eq!(a.pid, Some(999_999_999));
        assert_eq!(a.status, AgentStatus::Running);
    }

    #[test]
    fn given_dead_resumable_agent_should_mark_suspended() {
        let tmp = project_with(vec![agent("ag-1", "claude", Some("sid-1"))]);

        Engine::reconcile_agents_on_init(&root(&tmp), |_| false);

        let a = read(&tmp, "ag-1");
        assert!(a.suspended);
        assert_eq!(a.pid, None);
        assert_eq!(a.status, AgentStatus::Running);
    }

    #[test]
    fn given_dead_agent_without_session_should_mark_broken() {
        let tmp = project_with(vec![agent("ag-1", "terminal", None)]);

        Engine::reconcile_agents_on_init(&root(&tmp), |_| false);

        let a = read(&tmp, "ag-1");
        assert!(!a.suspended);
        assert_eq!(a.status, AgentStatus::Broken);
    }

    #[test]
    fn given_agent_becoming_live_after_reconcile_starts_should_leave_it_running() {
        let tmp = project_with(vec![agent("ag-1", "terminal", None)]);
        // Not live when reconcile first looks, live by the time it writes
        // (spawned or resumed in between).
        let checks = std::cell::Cell::new(0);
        let is_live = |_: &str| {
            checks.set(checks.get() + 1);
            checks.get() > 1
        };

        Engine::reconcile_agents_on_init(&root(&tmp), is_live);

        let a = read(&tmp, "ag-1");
        assert_eq!(a.status, AgentStatus::Running);
        assert!(!a.suspended);
    }

    #[test]
    fn given_pid_reused_by_unrelated_process_should_not_be_orphan() {
        // Our own pid is alive but its command line has no session id.
        assert!(!Engine::is_orphaned_session(
            std::process::id(),
            "not-a-session-id-xyz"
        ));
    }

    #[test]
    fn given_dead_resumable_agent_on_first_status_should_mark_suspended_not_broken() {
        let tmp = project_with(vec![
            agent("ag-1", "claude", Some("sid-1")),
            agent("ag-2", "terminal", None),
        ]);

        Engine::reap_stale_agents(&root(&tmp));

        let resumable = read(&tmp, "ag-1");
        assert!(resumable.suspended);
        assert_eq!(resumable.status, AgentStatus::Running);
        assert_eq!(read(&tmp, "ag-2").status, AgentStatus::Broken);
    }
}
