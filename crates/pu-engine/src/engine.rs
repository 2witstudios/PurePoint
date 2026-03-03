use std::collections::{HashMap, HashSet};
use std::os::fd::OwnedFd;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::Mutex;

use pu_core::config;
use pu_core::error::PuError;
use pu_core::manifest;
use pu_core::paths;
use pu_core::protocol::{AgentStatusReport, KillTarget, Request, Response, PROTOCOL_VERSION};
use pu_core::types::{AgentEntry, AgentStatus, Manifest, WorktreeEntry, WorktreeStatus};

use crate::agent_monitor;
use crate::git;
use crate::output_buffer::OutputBuffer;
use crate::pty_manager::{AgentHandle, NativePtyHost, SpawnConfig};

pub struct Engine {
    start_time: Instant,
    pty_host: NativePtyHost,
    sessions: Arc<Mutex<HashMap<String, AgentHandle>>>,
    login_path: String,
    reaped_projects: Arc<std::sync::Mutex<HashSet<String>>>,
}

impl Engine {
    pub async fn new() -> Self {
        Self {
            start_time: Instant::now(),
            pty_host: NativePtyHost::new(),
            sessions: Arc::new(Mutex::new(HashMap::new())),
            login_path: Self::resolve_login_path().await,
            reaped_projects: Arc::new(std::sync::Mutex::new(HashSet::new())),
        }
    }

    async fn resolve_login_path() -> String {
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into());
        match tokio::process::Command::new(&shell)
            .args(["-l", "-i", "-c", "echo $PATH"])
            .stderr(std::process::Stdio::null())
            .output()
            .await
        {
            Ok(output) if output.status.success() => {
                String::from_utf8_lossy(&output.stdout).trim().to_string()
            }
            _ => std::env::var("PATH").unwrap_or_default(),
        }
    }

    pub async fn handle_request(&self, request: Request) -> Response {
        match request {
            Request::Health => self.handle_health().await,
            Request::Init { project_root } => self.handle_init(&project_root).await,
            Request::Shutdown => Response::ShuttingDown,
            Request::Status { project_root, agent_id } => {
                self.handle_status(&project_root, agent_id.as_deref()).await
            }
            Request::Spawn {
                project_root,
                prompt,
                agent,
                name,
                base,
                root,
                worktree,
            } => {
                self.handle_spawn(&project_root, &prompt, &agent, name, base, root, worktree)
                    .await
            }
            Request::Kill { project_root, target } => {
                self.handle_kill(&project_root, target).await
            }
            Request::Logs { agent_id, tail } => self.handle_logs(&agent_id, tail).await,
            Request::Attach { agent_id } => self.handle_attach(&agent_id).await,
            Request::Input { agent_id, data } => self.handle_input(&agent_id, &data).await,
            Request::Resize { agent_id, cols, rows } => {
                self.handle_resize(&agent_id, cols, rows).await
            }
        }
    }

    async fn handle_health(&self) -> Response {
        let sessions = self.sessions.lock().await;
        Response::HealthReport {
            pid: std::process::id(),
            uptime_seconds: self.start_time.elapsed().as_secs(),
            protocol_version: PROTOCOL_VERSION,
            projects: vec![],
            agent_count: sessions.len(),
        }
    }

    async fn handle_init(&self, project_root: &str) -> Response {
        let project_root = project_root.to_string();
        tokio::task::spawn_blocking(move || {
            let root = Path::new(&project_root);
            let pu_dir = paths::pu_dir(root);

            if paths::manifest_path(root).exists() {
                return Response::InitResult { created: false };
            }

            if let Err(e) = std::fs::create_dir_all(&pu_dir) {
                return Response::Error {
                    code: "IO_ERROR".into(),
                    message: format!("failed to create .pu directory: {e}"),
                };
            }

            let m = Manifest::new(project_root.clone());
            if let Err(e) = manifest::write_manifest(root, &m) {
                return Response::Error {
                    code: "IO_ERROR".into(),
                    message: format!("failed to write manifest: {e}"),
                };
            }

            if let Err(e) = config::write_default_config(root) {
                return Response::Error {
                    code: "IO_ERROR".into(),
                    message: format!("failed to write config: {e}"),
                };
            }

            Response::InitResult { created: true }
        })
        .await
        .unwrap_or_else(|e| Response::Error {
            code: "INTERNAL_ERROR".into(),
            message: format!("task join error: {e}"),
        })
    }

    async fn handle_status(&self, project_root: &str, agent_id: Option<&str>) -> Response {
        // On first status call per project, reap agents whose PIDs are dead.
        // This handles daemon restarts where the manifest has stale Running entries.
        let should_reap = {
            let mut reaped = self.reaped_projects.lock().unwrap();
            reaped.insert(project_root.to_string())
        }; // MutexGuard dropped here — before any .await
        if should_reap {
            let pr = project_root.to_string();
            tokio::task::spawn_blocking(move || Self::reap_stale_agents(&pr))
                .await
                .ok();
        }

        let m = match Self::read_manifest_async(project_root).await {
            Ok(m) => m,
            Err(e) => return Self::error_response(&e),
        };

        if let Some(id) = agent_id {
            match m.find_agent(id) {
                Some(loc) => {
                    let (agent, wt_id) = match loc {
                        pu_core::types::AgentLocation::Root(a) => (a, None),
                        pu_core::types::AgentLocation::Worktree { worktree, agent } => {
                            (agent, Some(worktree.id.clone()))
                        }
                    };
                    let sessions = self.sessions.lock().await;
                    let (status, exit_code, idle_seconds) =
                        self.live_agent_status_sync(id, agent, &sessions);
                    Response::AgentStatus(AgentStatusReport {
                        id: agent.id.clone(),
                        name: agent.name.clone(),
                        status,
                        pid: agent.pid,
                        exit_code,
                        idle_seconds,
                        worktree_id: wt_id,
                    })
                }
                None => Self::agent_not_found(id),
            }
        } else {
            // Compute live status for all agents (root + worktree)
            let sessions = self.sessions.lock().await;
            let agents = m
                .agents
                .values()
                .map(|a| {
                    let (status, exit_code, idle_seconds) =
                        self.live_agent_status_sync(&a.id, a, &sessions);
                    AgentStatusReport {
                        id: a.id.clone(),
                        name: a.name.clone(),
                        status,
                        pid: a.pid,
                        exit_code,
                        idle_seconds,
                        worktree_id: None,
                    }
                })
                .collect();
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
            Response::StatusReport { worktrees, agents }
        }
    }

    /// Compute live agent status from PTY state.
    /// Returns (status, exit_code, idle_seconds).
    fn live_agent_status_sync(
        &self,
        id: &str,
        agent: &AgentEntry,
        sessions: &HashMap<String, AgentHandle>,
    ) -> (AgentStatus, Option<i32>, Option<u64>) {
        match sessions.get(id) {
            Some(handle) => {
                let exit_code = *handle.exit_rx.borrow();
                let status = agent_monitor::effective_status(exit_code, &handle.output_buffer);
                let idle_seconds = Some(handle.output_buffer.idle_seconds());
                (status, exit_code, idle_seconds)
            }
            // No live session — use manifest (agent already exited/killed/etc.)
            None => (agent.status, agent.exit_code, None),
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn handle_spawn(
        &self,
        project_root: &str,
        prompt: &str,
        agent_type: &str,
        name: Option<String>,
        base: Option<String>,
        root: bool,
        worktree: Option<String>,
    ) -> Response {
        let root_path = Path::new(project_root);

        // Ensure initialized
        if !paths::manifest_path(root_path).exists() {
            return Response::Error {
                code: "NOT_INITIALIZED".into(),
                message: "not initialized — run `pu init` first".into(),
            };
        }

        // Resolve agent config (strict: surface YAML parse errors)
        let cfg = match config::load_config_strict(root_path) {
            Ok(c) => c,
            Err(e) => {
                return Response::Error {
                    code: "CONFIG_ERROR".into(),
                    message: format!("failed to load config: {e}"),
                };
            }
        };
        let agent_cfg = match config::resolve_agent(&cfg, agent_type) {
            Some(c) => c.clone(),
            None => {
                return Response::Error {
                    code: "INVALID_ARGUMENT".into(),
                    message: format!("unknown agent type: {agent_type}"),
                };
            }
        };

        let agent_id = pu_core::id::agent_id();
        let agent_name = name.unwrap_or_else(|| agent_id.clone());
        let base_branch = base.unwrap_or_else(|| "HEAD".into());

        // Build command with prompt
        let mut cmd_parts: Vec<String> = agent_cfg
            .command
            .split_whitespace()
            .map(String::from)
            .collect();
        let command = cmd_parts.remove(0);
        // Resolve "shell" sentinel to user's login shell
        let command = if command == "shell" {
            std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into())
        } else {
            command
        };
        let mut args = cmd_parts;

        // Add agent-type-specific flags (not stored in config — engine concern)
        match agent_type {
            "claude" => {
                if !args.contains(&"--dangerously-skip-permissions".to_string()) {
                    args.insert(0, "--dangerously-skip-permissions".into());
                }
            }
            "codex" => {
                if !args.contains(&"--full-auto".to_string()) {
                    args.insert(0, "--full-auto".into());
                }
            }
            _ => {}
        }

        // Generate session ID for claude agents (enables resume via --resume)
        let session_id = if agent_type == "claude" {
            let id = pu_core::id::session_id();
            args.push("--session-id".into());
            args.push(id.clone());
            Some(id)
        } else {
            None
        };

        if !prompt.is_empty() {
            if let Some(flag) = &agent_cfg.prompt_flag {
                args.push(flag.clone());
            }
            args.push(prompt.to_string());
        }

        // Determine working directory
        let (cwd, worktree_id) = if root || worktree.is_some() {
            // Spawn in project root or existing worktree
            let wt_id = worktree.clone();
            let dir = if let Some(ref wt) = worktree {
                paths::worktree_path(root_path, wt)
                    .to_string_lossy()
                    .to_string()
            } else {
                project_root.to_string()
            };
            (dir, wt_id)
        } else {
            // Create new worktree
            let wt_id = pu_core::id::worktree_id();
            let wt_path = paths::worktree_path(root_path, &wt_id);
            let branch = format!("pu/{agent_name}");

            if let Err(e) = git::create_worktree(root_path, &wt_path, &branch, &base_branch).await
            {
                return Response::Error {
                    code: "SPAWN_FAILED".into(),
                    message: format!("failed to create worktree: {e}"),
                };
            }
            (wt_path.to_string_lossy().to_string(), Some(wt_id))
        };

        // Spawn PTY process
        let spawn_config = SpawnConfig {
            command,
            args,
            cwd: cwd.clone(),
            env: vec![
                ("PATH".into(), self.login_path.clone()),
                ("TERM".into(), "xterm-256color".into()),
                ("COLORTERM".into(), "truecolor".into()),
            ],
            env_remove: vec!["CLAUDECODE".into()],
            cols: 120,
            rows: 40,
        };

        // Track whether we created a new worktree (for rollback on failure)
        let created_worktree = !root && worktree.is_none() && worktree_id.is_some();

        let handle = match self.pty_host.spawn(spawn_config).await {
            Ok(h) => h,
            Err(e) => {
                if created_worktree {
                    self.rollback_worktree(root_path, worktree_id.as_deref()).await;
                }
                return Response::Error {
                    code: "SPAWN_FAILED".into(),
                    message: format!("failed to spawn process: {e}"),
                };
            }
        };

        let pid = handle.pid;

        // Update manifest
        let agent_entry = AgentEntry {
            id: agent_id.clone(),
            name: agent_name.clone(),
            agent_type: agent_type.to_string(),
            status: AgentStatus::Running,
            prompt: Some(prompt.to_string()),
            started_at: chrono::Utc::now(),
            completed_at: None,
            exit_code: None,
            error: None,
            pid: Some(pid),
            session_id,
        };

        let wt_id_for_manifest = worktree_id.clone();
        let agent_id_clone = agent_id.clone();
        let manifest_result = manifest::update_manifest(root_path, move |mut m| {
            if let Some(ref wt_id) = wt_id_for_manifest {
                // Add or update worktree entry
                let wt_entry = m.worktrees.entry(wt_id.clone()).or_insert_with(|| {
                    WorktreeEntry {
                        id: wt_id.clone(),
                        name: agent_name.clone(),
                        path: cwd.clone(),
                        branch: format!("pu/{agent_name}"),
                        base_branch: Some(base_branch.clone()),
                        status: WorktreeStatus::Active,
                        agents: HashMap::new(),
                        created_at: chrono::Utc::now(),
                        merged_at: None,
                    }
                });
                wt_entry.agents.insert(agent_id_clone, agent_entry);
            } else {
                m.agents.insert(agent_id_clone, agent_entry);
            }
            m
        });

        if let Err(e) = manifest_result {
            // Rollback: kill process and remove worktree
            self.pty_host
                .kill(&handle, Duration::from_secs(2))
                .await
                .ok();
            if created_worktree {
                self.rollback_worktree(root_path, worktree_id.as_deref()).await;
            }
            return Response::Error {
                code: "SPAWN_FAILED".into(),
                message: format!("failed to update manifest: {e}"),
            };
        }

        // Store handle in session map
        self.sessions.lock().await.insert(agent_id.clone(), handle);

        Response::SpawnResult {
            worktree_id,
            agent_id,
            status: AgentStatus::Running,
        }
    }

    async fn handle_kill(&self, project_root: &str, target: KillTarget) -> Response {
        let m = match Self::read_manifest_async(project_root).await {
            Ok(m) => m,
            Err(e) => return Self::error_response(&e),
        };

        let agent_ids: Vec<String> = match &target {
            KillTarget::Agent(id) => vec![id.clone()],
            KillTarget::Worktree(wt_id) => {
                match m.worktrees.get(wt_id) {
                    Some(wt) => wt.agents.keys().cloned().collect(),
                    None => {
                        return Response::Error {
                            code: "WORKTREE_NOT_FOUND".into(),
                            message: format!("worktree {wt_id} not found"),
                        };
                    }
                }
            }
            KillTarget::All => {
                let mut ids: Vec<String> = m.agents.keys().cloned().collect();
                for wt in m.worktrees.values() {
                    ids.extend(wt.agents.keys().cloned());
                }
                ids
            }
        };

        // Extract handles from session map, then DROP the lock before killing.
        // Killing can take up to 5s per agent — holding the mutex would block
        // all other operations (status, logs, attach).
        let handles_to_kill: Vec<(String, AgentHandle)> = {
            let mut sessions = self.sessions.lock().await;
            agent_ids
                .iter()
                .filter_map(|id| sessions.remove(id).map(|h| (id.clone(), h)))
                .collect()
        };
        // Lock is dropped here.

        let mut exit_codes = HashMap::new();
        for (id, handle) in &handles_to_kill {
            let state = self
                .pty_host
                .kill(handle, Duration::from_secs(5))
                .await
                .ok();
            exit_codes.insert(id.clone(), state.and_then(|s| s.exit_code));
        }

        // Update manifest: remove all targeted agents (off async runtime)
        let killed = agent_ids.clone();
        let killed_ids = killed.clone();
        let pr = project_root.to_string();
        tokio::task::spawn_blocking(move || {
            manifest::update_manifest(Path::new(&pr), move |mut m| {
                for id in &killed_ids {
                    m.agents.remove(id);
                    for wt in m.worktrees.values_mut() {
                        wt.agents.remove(id);
                    }
                }
                m
            })
            .ok();
        })
        .await
        .ok();

        Response::KillResult { killed, exit_codes }
    }

    async fn handle_logs(&self, agent_id: &str, tail: usize) -> Response {
        let buf = {
            let sessions = self.sessions.lock().await;
            match sessions.get(agent_id) {
                Some(handle) => handle.output_buffer.clone(),
                None => return Self::agent_not_found(agent_id),
            }
        };
        let data = buf.read_tail(tail);
        Response::LogsResult {
            agent_id: agent_id.to_string(),
            data: String::from_utf8_lossy(&data).to_string(),
        }
    }

    async fn handle_attach(&self, agent_id: &str) -> Response {
        let sessions = self.sessions.lock().await;
        match sessions.get(agent_id) {
            Some(handle) => Response::AttachReady {
                buffered_bytes: handle.output_buffer.len(),
            },
            None => Self::agent_not_found(agent_id),
        }
    }

    async fn handle_input(&self, agent_id: &str, data: &[u8]) -> Response {
        // Clone the fd Arc under the lock, then drop the lock before the blocking write
        let master_fd = {
            let sessions = self.sessions.lock().await;
            match sessions.get(agent_id) {
                Some(handle) => handle.master_fd(),
                None => return Self::agent_not_found(agent_id),
            }
        };
        match self.pty_host.write_to_fd(&master_fd, data).await {
            Ok(()) => Response::Ok,
            Err(e) => Response::Error {
                code: "IO_ERROR".into(),
                message: format!("write failed: {e}"),
            },
        }
    }

    async fn handle_resize(&self, agent_id: &str, cols: u16, rows: u16) -> Response {
        // Clone the fd Arc under the lock, then drop the lock before the blocking ioctl
        let master_fd = {
            let sessions = self.sessions.lock().await;
            match sessions.get(agent_id) {
                Some(handle) => handle.master_fd(),
                None => return Self::agent_not_found(agent_id),
            }
        };
        match self.pty_host.resize_fd(&master_fd, cols, rows).await {
            Ok(()) => Response::Ok,
            Err(e) => Response::Error {
                code: "IO_ERROR".into(),
                message: format!("resize failed: {e}"),
            },
        }
    }

    /// Write data to a PTY fd via the pty host (avoids duplicating unsafe write logic).
    pub async fn write_to_pty(&self, fd: &Arc<OwnedFd>, data: &[u8]) -> Result<(), std::io::Error> {
        self.pty_host.write_to_fd(fd, data).await
    }

    /// Resize a PTY fd via the pty host (avoids duplicating unsafe ioctl logic).
    pub async fn resize_pty(&self, fd: &Arc<OwnedFd>, cols: u16, rows: u16) -> Result<(), std::io::Error> {
        self.pty_host.resize_fd(fd, cols, rows).await
    }

    /// Return the output buffer and master PTY fd for an agent, if it has an active session.
    pub async fn get_attach_handles(
        &self,
        agent_id: &str,
    ) -> Option<(Arc<OutputBuffer>, Arc<OwnedFd>)> {
        let sessions = self.sessions.lock().await;
        sessions
            .get(agent_id)
            .map(|h| (h.output_buffer.clone(), h.master_fd()))
    }

    // --- Helpers ---

    fn is_pid_alive(pid: u32) -> bool {
        // kill(pid, 0) checks if the process exists without sending a signal
        unsafe { libc::kill(pid as i32, 0) == 0 }
    }

    /// Scan the manifest for Running/Idle agents whose PID is dead, mark them Lost.
    /// Called once per project on the first status request after daemon (re)start.
    fn reap_stale_agents(project_root: &str) {
        let root = Path::new(project_root);
        let m = match manifest::read_manifest(root) {
            Ok(m) => m,
            Err(_) => return,
        };

        let needs_reap = |agent: &AgentEntry| -> bool {
            matches!(agent.status, AgentStatus::Running | AgentStatus::Idle)
                && agent.pid.map_or(true, |pid| !Self::is_pid_alive(pid))
        };

        let has_stale = m.agents.values().any(|a| needs_reap(a))
            || m.worktrees.values().any(|wt| wt.agents.values().any(|a| needs_reap(a)));

        if !has_stale {
            return;
        }

        manifest::update_manifest(root, move |mut m| {
            for agent in m.agents.values_mut() {
                if matches!(agent.status, AgentStatus::Running | AgentStatus::Idle)
                    && agent.pid.map_or(true, |pid| !Self::is_pid_alive(pid))
                {
                    agent.status = AgentStatus::Lost;
                    agent.completed_at = Some(chrono::Utc::now());
                }
            }
            for wt in m.worktrees.values_mut() {
                for agent in wt.agents.values_mut() {
                    if matches!(agent.status, AgentStatus::Running | AgentStatus::Idle)
                        && agent.pid.map_or(true, |pid| !Self::is_pid_alive(pid))
                    {
                        agent.status = AgentStatus::Lost;
                        agent.completed_at = Some(chrono::Utc::now());
                    }
                }
            }
            m
        })
        .ok();
    }

    async fn rollback_worktree(&self, root_path: &Path, worktree_id: Option<&str>) {
        if let Some(wt_id) = worktree_id {
            let wt_path = paths::worktree_path(root_path, wt_id);
            git::remove_worktree(root_path, &wt_path).await.ok();
        }
    }

    fn agent_not_found(agent_id: &str) -> Response {
        Response::Error {
            code: "AGENT_NOT_FOUND".into(),
            message: format!("no active session for agent {agent_id}"),
        }
    }

    fn error_response(e: &PuError) -> Response {
        Response::Error {
            code: e.code().into(),
            message: e.to_string(),
        }
    }

    async fn read_manifest_async(project_root: &str) -> Result<Manifest, PuError> {
        let pr = project_root.to_string();
        tokio::task::spawn_blocking(move || manifest::read_manifest(Path::new(&pr)))
            .await
            .unwrap_or_else(|e| Err(PuError::Io(std::io::Error::other(e))))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::init_and_spawn;

    #[tokio::test(flavor = "current_thread")]
    async fn given_spawned_agent_should_return_attach_handles() {
        let (engine, agent_id, _tmp) = init_and_spawn().await;

        let handles = engine.get_attach_handles(&agent_id).await;
        assert!(handles.is_some(), "expected attach handles for spawned agent");

        let (buffer, _fd) = handles.unwrap();
        // Buffer exists and has a valid offset
        let _ = buffer.current_offset();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn given_unknown_agent_should_return_none() {
        let engine = Engine::new().await;
        let handles = engine.get_attach_handles("ag-nonexistent").await;
        assert!(handles.is_none());
    }
}
