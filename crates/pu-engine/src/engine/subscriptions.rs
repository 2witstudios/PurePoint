use pu_core::error::PuError;
use pu_core::manifest;
use pu_core::protocol::{AgentStatusReport, GridCommand, Response};
use pu_core::types::{AgentEntry, AgentLocation, AgentStatus, WorktreeEntry};
use std::path::Path;

use super::Engine;

impl Engine {
    // --- Grid ---

    pub(super) async fn handle_subscribe_grid(&self, project_root: &str) -> Response {
        self.ensure_grid_channel(project_root).await;
        Response::GridSubscribed
    }

    pub async fn handle_grid_command(&self, project_root: &str, command: GridCommand) -> Response {
        // For GetLayout, read the workspace layout file directly. `grid-layout.json` is
        // the pre-workspace format; the macOS app migrates it away on first load, so it is
        // only still here for a project that app has not opened since the change.
        if matches!(command, GridCommand::GetLayout) {
            let root = project_root.to_string();
            return match tokio::task::spawn_blocking(move || {
                let pu_dir = pu_core::paths::pu_dir(std::path::Path::new(&root));
                std::fs::read_to_string(pu_dir.join("workspaces.json"))
                    .or_else(|_| std::fs::read_to_string(pu_dir.join("grid-layout.json")))
            })
            .await
            {
                Ok(Ok(contents)) => match serde_json::from_str(&contents) {
                    Ok(layout) => Response::GridLayout { layout },
                    Err(e) => Response::Error {
                        code: "PARSE_ERROR".into(),
                        message: format!("invalid grid layout JSON: {e}"),
                    },
                },
                _ => Response::GridLayout {
                    layout: serde_json::Value::Null,
                },
            };
        }

        // Broadcast mutation commands to subscribers
        let channels = self.grid_channels.lock().await;
        if let Some(tx) = channels.get(project_root) {
            let _ = tx.send(command.clone());
        }
        Response::Ok
    }

    async fn ensure_grid_channel(&self, project_root: &str) {
        let mut channels = self.grid_channels.lock().await;
        channels
            .entry(project_root.to_string())
            .or_insert_with(|| tokio::sync::broadcast::channel(64).0);
    }

    /// Get a grid broadcast receiver for a project (used by IPC server for streaming).
    pub async fn subscribe_grid(
        &self,
        project_root: &str,
    ) -> tokio::sync::broadcast::Receiver<GridCommand> {
        let mut channels = self.grid_channels.lock().await;
        let tx = channels
            .entry(project_root.to_string())
            .or_insert_with(|| tokio::sync::broadcast::channel(64).0);
        tx.subscribe()
    }

    // --- Status Push ---

    pub(super) async fn handle_subscribe_status(&self, project_root: &str) -> Response {
        self.ensure_status_channel(project_root).await;
        Response::StatusSubscribed
    }

    async fn ensure_status_channel(&self, project_root: &str) {
        let mut channels = self.status_channels.lock().await;
        channels
            .entry(project_root.to_string())
            .or_insert_with(|| tokio::sync::broadcast::channel(64).0);
    }

    /// Get a status broadcast receiver for a project (used by IPC server for streaming).
    pub async fn subscribe_status(
        &self,
        project_root: &str,
    ) -> tokio::sync::broadcast::Receiver<()> {
        let mut channels = self.status_channels.lock().await;
        let tx = channels
            .entry(project_root.to_string())
            .or_insert_with(|| tokio::sync::broadcast::channel(64).0);
        tx.subscribe()
    }

    /// Notify all status subscribers that state has changed.
    pub(super) async fn notify_status_change(&self, project_root: &str) {
        let channels = self.status_channels.lock().await;
        if let Some(tx) = channels.get(project_root) {
            let _ = tx.send(());
        }
    }

    /// Record an agent's natural exit, then push a status update.
    ///
    /// The manifest is written before subscribers are notified: once the session
    /// handle is reaped, status falls back to the manifest, and an entry left at
    /// `Running` would make clients treat the agent as alive and reattach.
    ///
    /// Exits of processes killed by daemon shutdown are not recorded (see
    /// `kill_all_sessions`).
    ///
    /// Only the entry for this exact process lifetime is touched (same agent id
    /// and pid). Kill removes the entry and suspend clears the pid, so an
    /// intentional stop or a resumed process with a new pid is never overwritten.
    pub(super) fn watch_natural_exit(
        &self,
        project_root: &str,
        agent_id: &str,
        pid: u32,
        mut exit_rx: tokio::sync::watch::Receiver<Option<i32>>,
    ) {
        let status_channels = self.status_channels.clone();
        let shutdown_killed = self.shutdown_killed.clone();
        let project_root = project_root.to_string();
        let agent_id = agent_id.to_string();
        tokio::spawn(async move {
            // A dropped sender also means the process is gone.
            while exit_rx.borrow_and_update().is_none() {
                if exit_rx.changed().await.is_err() {
                    break;
                }
            }
            // The daemon killed it on the way down: leave the entry Running so
            // the next daemon's init marks it suspended and it resumes.
            if shutdown_killed
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .contains(&pid)
            {
                return;
            }
            let exit_code = *exit_rx.borrow();

            let pr = project_root.clone();
            tokio::task::spawn_blocking(move || {
                Self::record_natural_exit(Path::new(&pr), &agent_id, pid, exit_code)
            })
            .await
            .ok();

            if let Some(tx) = status_channels.lock().await.get(&project_root) {
                let _ = tx.send(());
            }
        });
    }

    fn record_natural_exit(project_root: &Path, agent_id: &str, pid: u32, exit_code: Option<i32>) {
        let is_this_lifetime = |a: &AgentEntry| {
            a.pid == Some(pid) && !a.suspended && matches!(a.status, AgentStatus::Running)
        };
        let Ok(m) = manifest::read_manifest(project_root) else {
            return;
        };
        let current = match m.find_agent(agent_id) {
            Some(AgentLocation::Root(a)) | Some(AgentLocation::Worktree { agent: a, .. }) => a,
            None => return,
        };
        if !is_this_lifetime(current) {
            return;
        }
        manifest::update_manifest(project_root, |mut m| {
            if let Some(agent) = m.find_agent_mut(agent_id)
                && is_this_lifetime(agent)
            {
                // Same status the live view reports for an exited process.
                agent.status = AgentStatus::Broken;
                agent.exit_code = exit_code;
                agent.completed_at = Some(chrono::Utc::now());
            }
            m
        })
        .ok();
    }

    /// Compute a full status report for a project (used by status push and handle_status).
    pub async fn compute_full_status(
        &self,
        project_root: &str,
    ) -> Result<(Vec<WorktreeEntry>, Vec<AgentStatusReport>), PuError> {
        let m = self.read_manifest_async(project_root).await?;
        let sessions = self.sessions.lock().await;
        let mut agents: Vec<AgentStatusReport> = m
            .agents
            .values()
            .map(|a| self.build_agent_status_report(a, &sessions, None))
            .collect();
        agents.sort_by_key(|a| a.started_at);
        let worktrees: Vec<WorktreeEntry> = m
            .worktrees
            .into_values()
            .map(|mut wt| {
                for agent in wt.agents.values_mut() {
                    let (status, exit_code, _idle) =
                        self.live_agent_status_sync(&agent.id, agent, &sessions);
                    agent.status = status;
                    agent.exit_code = exit_code;
                }
                wt
            })
            .collect();
        Ok((worktrees, agents))
    }
}

#[cfg(test)]
mod natural_exit_tests {
    use super::*;
    use crate::pty_manager::{AgentHandle, SpawnConfig};
    use pu_core::types::Manifest;
    use tempfile::TempDir;

    fn agent(id: &str, pid: Option<u32>, suspended: bool) -> AgentEntry {
        serde_json::from_value(serde_json::json!({
            "id": id,
            "name": id,
            "agentType": "claude",
            "status": "running",
            "startedAt": "2026-01-01T00:00:00Z",
            "pid": pid,
            "suspended": suspended,
        }))
        .unwrap()
    }

    fn project_with(entry: AgentEntry) -> TempDir {
        let tmp = TempDir::new().unwrap();
        let manifest_path = pu_core::paths::manifest_path(tmp.path());
        std::fs::create_dir_all(manifest_path.parent().unwrap()).unwrap();
        let mut m = Manifest::new(tmp.path().to_string_lossy().into_owned());
        m.agents.insert(entry.id.clone(), entry);
        manifest::write_manifest(tmp.path(), &m).unwrap();
        tmp
    }

    fn read(tmp: &TempDir, id: &str) -> AgentEntry {
        manifest::read_manifest(tmp.path()).unwrap().agents[id].clone()
    }

    #[test]
    fn given_natural_exit_should_persist_broken_with_exit_code() {
        let tmp = project_with(agent("ag-1", Some(42), false));

        Engine::record_natural_exit(tmp.path(), "ag-1", 42, Some(0));

        let a = read(&tmp, "ag-1");
        assert_eq!(a.status, AgentStatus::Broken);
        assert_eq!(a.exit_code, Some(0));
        assert!(a.completed_at.is_some());
    }

    #[test]
    fn given_resumed_process_with_new_pid_should_not_touch_entry() {
        let tmp = project_with(agent("ag-1", Some(99), false));

        Engine::record_natural_exit(tmp.path(), "ag-1", 42, Some(0));

        let a = read(&tmp, "ag-1");
        assert_eq!(a.status, AgentStatus::Running);
        assert_eq!(a.exit_code, None);
    }

    #[test]
    fn given_suspended_agent_should_not_touch_entry() {
        let tmp = project_with(agent("ag-1", None, true));

        Engine::record_natural_exit(tmp.path(), "ag-1", 42, Some(0));

        let a = read(&tmp, "ag-1");
        assert_eq!(a.status, AgentStatus::Running);
        assert!(a.suspended);
    }

    #[test]
    fn given_killed_agent_removed_from_manifest_should_not_recreate_it() {
        let tmp = project_with(agent("ag-1", Some(42), false));
        manifest::update_manifest(tmp.path(), |mut m| {
            m.agents.shift_remove("ag-1");
            m
        })
        .unwrap();

        Engine::record_natural_exit(tmp.path(), "ag-1", 42, Some(0));

        assert!(
            manifest::read_manifest(tmp.path())
                .unwrap()
                .find_agent("ag-1")
                .is_none()
        );
    }

    async fn spawn_process(engine: &Engine, command: &str, args: &[&str]) -> AgentHandle {
        engine
            .pty_host
            .spawn(SpawnConfig {
                command: command.into(),
                args: args.iter().map(|a| a.to_string()).collect(),
                cwd: "/tmp".into(),
                env: vec![],
                env_remove: vec![],
                cols: 80,
                rows: 24,
            })
            .await
            .unwrap()
    }

    /// Poll the manifest until `id` reaches `want`, failing after 5s.
    async fn wait_for_status(tmp: &TempDir, id: &str, want: AgentStatus) {
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
        while read(tmp, id).status != want {
            assert!(
                tokio::time::Instant::now() < deadline,
                "{id} never reached {want:?}"
            );
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    }

    async fn wait_for_exit(handle: &AgentHandle) {
        let mut rx = handle.exit_rx.clone();
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            while rx.borrow_and_update().is_none() {
                if rx.changed().await.is_err() {
                    break;
                }
            }
        })
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn given_agent_killed_by_shutdown_should_leave_agent_resumable() {
        let engine = Engine::new();
        let handle = spawn_process(&engine, "/bin/sleep", &["30"]).await;
        let pid = handle.pid;
        let exit_rx = handle.exit_rx.clone();
        let tmp = project_with(agent("ag-1", Some(pid), false));
        // A control agent outside shutdown: once its exit is recorded, the
        // watchers have run, so ag-1 staying Running is not just a slow watcher.
        let control = project_with(agent("ag-2", Some(42), false));
        let (control_tx, control_rx) = tokio::sync::watch::channel(None);
        engine.watch_natural_exit(&tmp.path().to_string_lossy(), "ag-1", pid, exit_rx.clone());
        engine.watch_natural_exit(&control.path().to_string_lossy(), "ag-2", 42, control_rx);
        engine.sessions.lock().await.insert("ag-1".into(), handle);

        engine
            .kill_all_sessions(std::time::Duration::from_secs(2))
            .await;
        assert!(
            exit_rx.borrow().is_some(),
            "shutdown should have stopped ag-1"
        );
        control_tx.send(Some(0)).unwrap();
        wait_for_status(&control, "ag-2", AgentStatus::Broken).await;

        let a = read(&tmp, "ag-1");
        assert_eq!(a.status, AgentStatus::Running);
        assert_eq!(a.exit_code, None);
    }

    #[tokio::test]
    async fn given_agent_exited_before_shutdown_with_watcher_pending_should_record_broken() {
        let engine = Engine::new();
        let handle = spawn_process(&engine, "/usr/bin/true", &[]).await;
        let pid = handle.pid;
        let exit_rx = handle.exit_rx.clone();
        let tmp = project_with(agent("ag-1", Some(pid), false));
        wait_for_exit(&handle).await;
        engine.sessions.lock().await.insert("ag-1".into(), handle);

        engine
            .kill_all_sessions(std::time::Duration::from_secs(2))
            .await;
        // The watcher only gets to run after teardown began.
        engine.watch_natural_exit(&tmp.path().to_string_lossy(), "ag-1", pid, exit_rx);

        wait_for_status(&tmp, "ag-1", AgentStatus::Broken).await;
    }

    #[tokio::test]
    async fn given_exit_outside_shutdown_should_record_broken() {
        let tmp = project_with(agent("ag-1", Some(42), false));
        let engine = Engine::new();
        let (tx, rx) = tokio::sync::watch::channel(None);
        engine.watch_natural_exit(&tmp.path().to_string_lossy(), "ag-1", 42, rx);

        tx.send(Some(0)).unwrap();

        wait_for_status(&tmp, "ag-1", AgentStatus::Broken).await;
    }
}
