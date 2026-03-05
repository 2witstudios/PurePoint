use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::os::fd::OwnedFd;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::Mutex;

use indexmap::IndexMap;
use pu_core::config;
use pu_core::error::PuError;
use pu_core::manifest;
use pu_core::paths;
use pu_core::protocol::{
    AgentStatusReport, GridCommand, KillTarget, PROTOCOL_VERSION, Request, Response, SuspendTarget,
};
use pu_core::types::{AgentEntry, AgentStatus, Manifest, WorktreeEntry, WorktreeStatus};
use tokio::sync::OnceCell;

use crate::agent_monitor;
use crate::daemon_lifecycle;
use crate::git;
use crate::output_buffer::OutputBuffer;
use crate::pty_manager::{AgentHandle, NativePtyHost, SpawnConfig};

pub struct Engine {
    start_time: Instant,
    pty_host: NativePtyHost,
    sessions: Arc<Mutex<HashMap<String, AgentHandle>>>,
    login_path: Arc<OnceCell<String>>,
    reaped_projects: Arc<std::sync::Mutex<HashSet<String>>>,
    /// Per-project broadcast channels for grid commands.
    grid_channels: Arc<Mutex<HashMap<String, tokio::sync::broadcast::Sender<GridCommand>>>>,
    /// Per-project broadcast channels for status push updates.
    status_channels: Arc<Mutex<HashMap<String, tokio::sync::broadcast::Sender<()>>>>,
}

impl Default for Engine {
    fn default() -> Self {
        Self::new()
    }
}

impl Engine {
    pub fn new() -> Self {
        Self {
            start_time: Instant::now(),
            pty_host: NativePtyHost::new(),
            sessions: Arc::new(Mutex::new(HashMap::new())),
            login_path: Arc::new(OnceCell::new()),
            reaped_projects: Arc::new(std::sync::Mutex::new(HashSet::new())),
            grid_channels: Arc::new(Mutex::new(HashMap::new())),
            status_channels: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Start a background task that periodically removes session handles for
    /// processes that have exited naturally, and cleans up broadcast channels
    /// with no subscribers. Without this, HashMap entries leak.
    pub fn start_session_reaper(self: &Arc<Self>) {
        let sessions = self.sessions.clone();
        let grid_channels = self.grid_channels.clone();
        let status_channels = self.status_channels.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(30));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                interval.tick().await;

                // Reap dead sessions
                let dead_ids: Vec<String> = {
                    let sessions = sessions.lock().await;
                    sessions
                        .iter()
                        .filter(|(_, handle)| handle.exit_rx.borrow().is_some())
                        .map(|(id, _)| id.clone())
                        .collect()
                };
                if !dead_ids.is_empty() {
                    let mut sessions = sessions.lock().await;
                    for id in &dead_ids {
                        sessions.remove(id);
                    }
                    tracing::debug!(count = dead_ids.len(), "reaped dead session handles");
                }

                // Clean up grid channels with no subscribers
                {
                    let mut channels = grid_channels.lock().await;
                    channels.retain(|_, tx| tx.receiver_count() > 0);
                }

                // Clean up status channels with no subscribers
                {
                    let mut channels = status_channels.lock().await;
                    channels.retain(|_, tx| tx.receiver_count() > 0);
                }
            }
        });
    }

    async fn resolve_login_path() -> String {
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into());
        let base_path = match tokio::process::Command::new(&shell)
            .args(["-li", "-c", "echo $PATH"])
            .stdin(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .output()
            .await
        {
            Ok(output) if output.status.success() => {
                String::from_utf8_lossy(&output.stdout).trim().to_string()
            }
            _ => std::env::var("PATH").unwrap_or_default(),
        };

        // Append common tool directories that may only appear in .zshrc/.bashrc
        // (guards against the missing-claude-binary issue from d60c911)
        let home = std::env::var("HOME").unwrap_or_default();
        let fallbacks = [
            format!("{home}/.local/bin"),
            format!("{home}/.cargo/bin"),
            "/usr/local/bin".to_string(),
            "/opt/homebrew/bin".to_string(),
        ];
        let mut path = base_path;
        for dir in fallbacks {
            if !path.split(':').any(|p| p == dir) {
                path = format!("{path}:{dir}");
            }
        }
        path
    }

    pub async fn handle_request(&self, request: Request) -> Response {
        match request {
            Request::Health => self.handle_health().await,
            Request::Init { project_root } => self.handle_init(&project_root).await,
            Request::Rename {
                project_root,
                agent_id,
                name,
            } => self.handle_rename(&project_root, &agent_id, &name).await,
            Request::Shutdown => Response::ShuttingDown,
            Request::Status {
                project_root,
                agent_id,
            } => self.handle_status(&project_root, agent_id.as_deref()).await,
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
            Request::Kill {
                project_root,
                target,
            } => self.handle_kill(&project_root, target).await,
            Request::Suspend {
                project_root,
                target,
            } => self.handle_suspend(&project_root, target).await,
            Request::Resume {
                project_root,
                agent_id,
            } => self.handle_resume(&project_root, &agent_id).await,
            Request::Logs { agent_id, tail } => self.handle_logs(&agent_id, tail).await,
            Request::Attach { agent_id } => self.handle_attach(&agent_id).await,
            Request::Input { agent_id, data } => self.handle_input(&agent_id, &data).await,
            Request::Resize {
                agent_id,
                cols,
                rows,
            } => self.handle_resize(&agent_id, cols, rows).await,
            Request::SubscribeGrid { project_root } => {
                self.handle_subscribe_grid(&project_root).await
            }
            Request::SubscribeStatus { project_root } => {
                self.handle_subscribe_status(&project_root).await
            }
            Request::GridCommand {
                project_root,
                command,
            } => self.handle_grid_command(&project_root, command).await,
            Request::DeleteWorktree {
                project_root,
                worktree_id,
            } => {
                self.handle_delete_worktree(&project_root, &worktree_id)
                    .await
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

            if let Err(e) = std::fs::create_dir_all(&pu_dir) {
                return Response::Error {
                    code: "IO_ERROR".into(),
                    message: format!("failed to create .pu directory: {e}"),
                };
            }

            // Atomic check-and-create via O_EXCL — prevents TOCTOU race
            let manifest_path = paths::manifest_path(root);
            let file = match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&manifest_path)
            {
                Ok(f) => f,
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    Self::reconcile_agents_on_init(&project_root);
                    return Response::InitResult { created: false };
                }
                Err(e) => {
                    return Response::Error {
                        code: "IO_ERROR".into(),
                        message: format!("failed to create manifest: {e}"),
                    };
                }
            };

            let m = Manifest::new(project_root.clone());
            let content = match serde_json::to_string_pretty(&m) {
                Ok(c) => c + "\n",
                Err(e) => {
                    let _ = std::fs::remove_file(&manifest_path);
                    return Response::Error {
                        code: "IO_ERROR".into(),
                        message: format!("failed to serialize manifest: {e}"),
                    };
                }
            };
            let mut file = file;
            if let Err(e) = file
                .write_all(content.as_bytes())
                .and_then(|_| file.sync_all())
            {
                let _ = std::fs::remove_file(&manifest_path);
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

        let m = match self.read_manifest_async(project_root).await {
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
                        agent_type: agent.agent_type.clone(),
                        status,
                        pid: agent.pid,
                        exit_code,
                        idle_seconds,
                        worktree_id: wt_id,
                        started_at: agent.started_at,
                        session_id: agent.session_id.clone(),
                        prompt: agent.prompt.clone(),
                        suspended: agent.suspended,
                    })
                }
                None => Self::agent_not_found(id),
            }
        } else {
            // Compute live status for all agents (root + worktree)
            let sessions = self.sessions.lock().await;
            let mut agents: Vec<AgentStatusReport> = m
                .agents
                .values()
                .map(|a| {
                    let (status, exit_code, idle_seconds) =
                        self.live_agent_status_sync(&a.id, a, &sessions);
                    AgentStatusReport {
                        id: a.id.clone(),
                        name: a.name.clone(),
                        agent_type: a.agent_type.clone(),
                        status,
                        pid: a.pid,
                        exit_code,
                        idle_seconds,
                        worktree_id: None,
                        started_at: a.started_at,
                        session_id: a.session_id.clone(),
                        prompt: a.prompt.clone(),
                        suspended: a.suspended,
                    }
                })
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
        let creating_new_worktree = !root && worktree.is_none();
        let agent_name = if creating_new_worktree {
            // Worktree spawns require a user-provided name (becomes the branch slug)
            let raw = match name {
                Some(n) => n,
                None => {
                    return Response::Error {
                        code: "INVALID_ARGUMENT".into(),
                        message: "worktree spawn requires a name".into(),
                    };
                }
            };
            let normalized = pu_core::id::normalize_worktree_name(&raw);
            if normalized.is_empty() {
                return Response::Error {
                    code: "INVALID_ARGUMENT".into(),
                    message: "worktree spawn requires a name".into(),
                };
            }
            normalized
        } else {
            // Root agents and existing-worktree agents get auto-generated names
            name.unwrap_or_else(pu_core::id::root_agent_name)
        };
        let base_branch = base.unwrap_or_else(|| "HEAD".into());

        // Build command with prompt
        let (command, cmd_args) = match Self::parse_agent_command(&agent_cfg, agent_type) {
            Ok(v) => v,
            Err(e) => return e,
        };
        let mut args = cmd_args;

        // Add agent-type-specific flags (not stored in config — engine concern)
        match agent_type {
            "claude" => {
                if !args.iter().any(|a| a == "--dangerously-skip-permissions") {
                    args.insert(0, "--dangerously-skip-permissions".into());
                }
            }
            "codex" => {
                if !args.iter().any(|a| a == "--full-auto") {
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

            if let Err(e) = git::create_worktree(root_path, &wt_path, &branch, &base_branch).await {
                return Response::Error {
                    code: "SPAWN_FAILED".into(),
                    message: format!("failed to create worktree: {e}"),
                };
            }

            // Copy env files (e.g., .env, .env.local) into new worktree
            for env_file in &cfg.env_files {
                let src = root_path.join(env_file);
                if src.exists() {
                    let dst = wt_path.join(env_file);
                    if let Err(e) = tokio::fs::copy(&src, &dst).await {
                        tracing::warn!("failed to copy {env_file} to worktree: {e}");
                    }
                }
            }

            (wt_path.to_string_lossy().to_string(), Some(wt_id))
        };

        // Spawn PTY process
        let spawn_config = SpawnConfig {
            command,
            args,
            cwd: cwd.clone(),
            env: vec![
                ("PATH".into(), self.agent_path().await),
                ("TERM".into(), "xterm-256color".into()),
                ("COLORTERM".into(), "truecolor".into()),
            ],
            env_remove: vec!["CLAUDECODE".into()],
            cols: 120,
            rows: 40,
        };

        // Track whether we created a new worktree (for rollback on failure)
        let created_worktree = !root && worktree.is_none() && worktree_id.is_some();
        let rollback_branch = if created_worktree {
            Some(format!("pu/{agent_name}"))
        } else {
            None
        };

        let handle = match self.pty_host.spawn(spawn_config).await {
            Ok(h) => h,
            Err(e) => {
                if created_worktree {
                    self.rollback_worktree(
                        root_path,
                        worktree_id.as_deref(),
                        rollback_branch.as_deref(),
                    )
                    .await;
                }
                return Response::Error {
                    code: "SPAWN_FAILED".into(),
                    message: format!("failed to spawn process: {e}"),
                };
            }
        };

        let pid = handle.pid;

        // Store handle in session map BEFORE writing manifest.
        // ManifestWatcher in Swift fires on manifest write and immediately
        // tries to attach — the session must already be in the map.
        self.sessions.lock().await.insert(agent_id.clone(), handle);

        // Update manifest
        let agent_entry = AgentEntry {
            id: agent_id.clone(),
            name: agent_name.clone(),
            agent_type: agent_type.to_string(),
            status: AgentStatus::Streaming,
            prompt: Some(prompt.to_string()),
            started_at: chrono::Utc::now(),
            completed_at: None,
            exit_code: None,
            error: None,
            pid: Some(pid),
            session_id,
            suspended_at: None,
            suspended: false,
        };

        let wt_id_for_manifest = worktree_id.clone();
        let agent_id_clone = agent_id.clone();
        let manifest_result = manifest::update_manifest(root_path, move |mut m| {
            if let Some(ref wt_id) = wt_id_for_manifest {
                // Add or update worktree entry
                let wt_entry = m
                    .worktrees
                    .entry(wt_id.clone())
                    .or_insert_with(|| WorktreeEntry {
                        id: wt_id.clone(),
                        name: agent_name.clone(),
                        path: cwd.clone(),
                        branch: format!("pu/{agent_name}"),
                        base_branch: Some(base_branch.clone()),
                        status: WorktreeStatus::Active,
                        agents: IndexMap::new(),
                        created_at: chrono::Utc::now(),
                        merged_at: None,
                    });
                wt_entry.agents.insert(agent_id_clone, agent_entry);
            } else {
                m.agents.insert(agent_id_clone, agent_entry);
            }
            m
        });

        match manifest_result {
            Err(e) => {
                // Rollback: remove session and kill process
                if let Some(handle) = self.sessions.lock().await.remove(&agent_id) {
                    self.pty_host
                        .kill(&handle, Duration::from_secs(2))
                        .await
                        .ok();
                }
                if created_worktree {
                    self.rollback_worktree(
                        root_path,
                        worktree_id.as_deref(),
                        rollback_branch.as_deref(),
                    )
                    .await;
                }
                return Response::Error {
                    code: "SPAWN_FAILED".into(),
                    message: format!("failed to update manifest: {e}"),
                };
            }
            Ok(_) => {}
        }

        self.notify_status_change(project_root).await;

        Response::SpawnResult {
            worktree_id,
            agent_id,
            status: AgentStatus::Streaming,
        }
    }

    async fn handle_kill(&self, project_root: &str, target: KillTarget) -> Response {
        let m = match self.read_manifest_async(project_root).await {
            Ok(m) => m,
            Err(e) => return Self::error_response(&e),
        };

        let agent_ids: Vec<String> = match &target {
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

        Response::KillResult { killed, exit_codes }
    }

    async fn handle_delete_worktree(&self, project_root: &str, worktree_id: &str) -> Response {
        let m = match self.read_manifest_async(project_root).await {
            Ok(m) => m,
            Err(e) => return Self::error_response(&e),
        };

        let wt = match m.worktrees.get(worktree_id) {
            Some(wt) => wt.clone(),
            None => {
                return Response::Error {
                    code: "WORKTREE_NOT_FOUND".into(),
                    message: format!("worktree {worktree_id} not found"),
                };
            }
        };

        // 1. Kill all agents in the worktree
        let agent_ids: Vec<String> = wt.agents.keys().cloned().collect();
        let handles_to_kill: Vec<(String, AgentHandle)> = {
            let mut sessions = self.sessions.lock().await;
            agent_ids
                .iter()
                .filter_map(|id| sessions.remove(id).map(|h| (id.clone(), h)))
                .collect()
        };
        for (_, handle) in &handles_to_kill {
            self.pty_host
                .kill(handle, Duration::from_secs(5))
                .await
                .ok();
        }

        // 2. Remove git worktree directory
        let root_path = Path::new(project_root);
        let wt_path = paths::worktree_path(root_path, worktree_id);
        git::remove_worktree(root_path, &wt_path).await.ok();

        // 3. Delete local branch (soft-fail)
        let branch = wt.branch.clone();
        let branch_deleted = git::delete_local_branch(root_path, &branch).await.is_ok();

        // 4. Delete remote branch (soft-fail)
        let remote_deleted = git::delete_remote_branch(root_path, &branch).await.is_ok();

        // 5. Remove worktree from manifest
        let wt_id = worktree_id.to_string();
        let killed_agents = agent_ids.clone();
        let pr = project_root.to_string();
        tokio::task::spawn_blocking(move || {
            manifest::update_manifest(Path::new(&pr), move |mut m| {
                m.worktrees.shift_remove(&wt_id);
                m
            })
            .ok();
        })
        .await
        .ok();

        self.notify_status_change(project_root).await;

        Response::DeleteWorktreeResult {
            worktree_id: worktree_id.to_string(),
            killed_agents,
            branch_deleted,
            remote_deleted,
        }
    }

    async fn handle_rename(&self, project_root: &str, agent_id: &str, name: &str) -> Response {
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
                code: "INTERNAL".into(),
                message: format!("rename task failed: {e}"),
            },
        }
    }

    async fn handle_suspend(&self, project_root: &str, target: SuspendTarget) -> Response {
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

        // Extract handles from session map, then drop lock before killing
        let handles_to_kill: Vec<(String, AgentHandle)> = {
            let mut sessions = self.sessions.lock().await;
            agent_ids
                .iter()
                .filter_map(|id| sessions.remove(id).map(|h| (id.clone(), h)))
                .collect()
        };

        for (_id, handle) in &handles_to_kill {
            self.pty_host
                .kill(handle, Duration::from_secs(5))
                .await
                .ok();
        }

        // Update manifest: mark as suspended, clear pid, set suspended_at.
        // Status stays as-is (Waiting); suspended flag is metadata.
        let suspended = agent_ids.clone();
        let suspended_ids = suspended.clone();
        let pr = project_root.to_string();
        tokio::task::spawn_blocking(move || {
            manifest::update_manifest(Path::new(&pr), move |mut m| {
                let now = chrono::Utc::now();
                for id in &suspended_ids {
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

    async fn handle_resume(&self, project_root: &str, agent_id: &str) -> Response {
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

        // 3. Construct resume command based on agent type
        let (command, args, session_id) = match self.build_resume_command(
            &agent_entry.agent_type,
            &agent_cfg,
            agent_entry.session_id.as_deref(),
        ) {
            Ok(result) => result,
            Err(response) => return response,
        };

        // 4. Spawn PTY process
        let spawn_config = SpawnConfig {
            command,
            args,
            cwd,
            env: vec![
                ("PATH".into(), self.agent_path().await),
                ("TERM".into(), "xterm-256color".into()),
                ("COLORTERM".into(), "truecolor".into()),
            ],
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

        // 5. Update manifest: Suspended → Running, new PID
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
            // Rollback: kill the resumed process
            self.pty_host
                .kill(&handle, Duration::from_secs(2))
                .await
                .ok();
            return Response::Error {
                code: "RESUME_FAILED".into(),
                message: format!("failed to update manifest: {e}"),
            };
        }

        // 6. Store handle in session map
        self.sessions
            .lock()
            .await
            .insert(agent_id.to_string(), handle);

        self.notify_status_change(project_root).await;

        Response::ResumeResult {
            agent_id: agent_id.to_string(),
            status: AgentStatus::Streaming,
        }
    }

    /// Construct the resume command for a given agent type.
    /// Returns Ok((command, args, session_id)) or Err(Response) on failure.
    #[allow(clippy::result_large_err)]
    fn build_resume_command(
        &self,
        agent_type: &str,
        agent_cfg: &pu_core::types::AgentConfig,
        session_id: Option<&str>,
    ) -> Result<(String, Vec<String>, Option<String>), Response> {
        match agent_type {
            "claude" => {
                let sid = session_id.ok_or_else(|| Response::Error {
                    code: "RESUME_FAILED".into(),
                    message: "cannot resume Claude agent: no session_id preserved".into(),
                })?;
                let args = vec![
                    "--dangerously-skip-permissions".into(),
                    "--resume".into(),
                    sid.to_string(),
                ];
                Ok(("claude".into(), args, Some(sid.to_string())))
            }
            "codex" => {
                let args = vec!["resume".into(), "--last".into(), "--full-auto".into()];
                Ok(("codex".into(), args, None))
            }
            "opencode" => {
                let args = vec!["--continue".into()];
                Ok(("opencode".into(), args, None))
            }
            _ => {
                // Terminal / unknown: fresh shell in same directory
                let (command, args) = Self::parse_agent_command(agent_cfg, agent_type)?;
                Ok((command, args, None))
            }
        }
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
        let text = String::from_utf8_lossy(&data);
        if let std::borrow::Cow::Owned(_) = &text {
            tracing::warn!(
                agent_id,
                "logs output contained non-UTF-8 bytes (lossy conversion applied)"
            );
        }
        Response::LogsResult {
            agent_id: agent_id.to_string(),
            data: text.into_owned(),
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
    pub async fn resize_pty(
        &self,
        fd: &Arc<OwnedFd>,
        cols: u16,
        rows: u16,
    ) -> Result<(), std::io::Error> {
        self.pty_host.resize_fd(fd, cols, rows).await
    }

    /// Return the output buffer, master PTY fd, and exit receiver for an agent,
    /// if it has an active session.
    pub async fn get_attach_handles(
        &self,
        agent_id: &str,
    ) -> Option<(
        Arc<OutputBuffer>,
        Arc<OwnedFd>,
        tokio::sync::watch::Receiver<Option<i32>>,
    )> {
        let sessions = self.sessions.lock().await;
        sessions
            .get(agent_id)
            .map(|h| (h.output_buffer.clone(), h.master_fd(), h.exit_rx.clone()))
    }

    /// Build the PATH env value for spawned agents.
    /// Prepends ~/.pu/bin so agents can find the `pu` CLI.
    /// Login PATH is resolved lazily on first spawn (absorbed into spawn latency).
    async fn agent_path(&self) -> String {
        let login_path = self.login_path.get_or_init(Self::resolve_login_path).await;
        match paths::global_pu_dir() {
            Ok(pu_dir) => {
                let bin_dir = pu_dir.join("bin");
                format!("{}:{}", bin_dir.display(), login_path)
            }
            Err(_) => login_path.clone(),
        }
    }

    // --- Grid ---

    async fn handle_subscribe_grid(&self, project_root: &str) -> Response {
        let mut channels = self.grid_channels.lock().await;
        channels
            .entry(project_root.to_string())
            .or_insert_with(|| tokio::sync::broadcast::channel(64).0);
        Response::GridSubscribed
    }

    pub async fn handle_grid_command(&self, project_root: &str, command: GridCommand) -> Response {
        // For GetLayout, read the grid-layout.json directly
        if matches!(command, GridCommand::GetLayout) {
            let root = project_root.to_string();
            return match tokio::task::spawn_blocking(move || {
                let path = format!("{root}/.pu/grid-layout.json");
                std::fs::read_to_string(path)
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

    // --- Helpers ---

    /// Parse an agent config's command string into (program, args), resolving
    /// the "shell" sentinel to the user's login shell.
    #[allow(clippy::result_large_err)]
    fn parse_agent_command(
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

    fn is_pid_alive(pid: u32) -> bool {
        daemon_lifecycle::is_process_alive(pid)
    }

    /// On daemon restart, reconcile agents that appear alive in the manifest but have no
    /// live process. Resumable agents (claude, codex, opencode) with a session_id get marked
    /// suspended so the Swift side can auto-resume them. Others get marked Broken.
    /// Called synchronously inside handle_init so state is correct before the first status read.
    fn reconcile_agents_on_init(project_root: &str) {
        let root = Path::new(project_root);
        let m = match manifest::read_manifest(root) {
            Ok(m) => m,
            Err(_) => return,
        };

        let is_resumable = |t: &str| matches!(t, "claude" | "codex" | "opencode");
        let is_stale = |a: &AgentEntry| {
            !a.suspended && matches!(a.status, AgentStatus::Streaming | AgentStatus::Waiting)
        };

        let has_stale = m.agents.values().any(is_stale)
            || m.worktrees
                .values()
                .any(|wt| wt.agents.values().any(is_stale));
        if !has_stale {
            return;
        }

        let now = chrono::Utc::now();
        manifest::update_manifest(root, move |mut m| {
            for agent in m.agents.values_mut().chain(
                m.worktrees
                    .values_mut()
                    .flat_map(|wt| wt.agents.values_mut()),
            ) {
                if !agent.suspended
                    && matches!(agent.status, AgentStatus::Streaming | AgentStatus::Waiting)
                {
                    if agent.session_id.is_some() && is_resumable(&agent.agent_type) {
                        agent.status = AgentStatus::Waiting;
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
    fn reap_stale_agents(project_root: &str) {
        let root = Path::new(project_root);
        let m = match manifest::read_manifest(root) {
            Ok(m) => m,
            Err(_) => return,
        };

        let needs_reap = |agent: &AgentEntry| -> bool {
            !agent.suspended
                && matches!(agent.status, AgentStatus::Streaming | AgentStatus::Waiting)
                && agent.pid.is_none_or(|pid| !Self::is_pid_alive(pid))
        };

        let has_stale = m.agents.values().any(&needs_reap)
            || m.worktrees
                .values()
                .any(|wt| wt.agents.values().any(&needs_reap));

        if !has_stale {
            return;
        }

        manifest::update_manifest(root, move |mut m| {
            for agent in m.agents.values_mut() {
                if !agent.suspended
                    && matches!(agent.status, AgentStatus::Streaming | AgentStatus::Waiting)
                    && agent.pid.is_none_or(|pid| !Self::is_pid_alive(pid))
                {
                    agent.status = AgentStatus::Broken;
                    agent.completed_at = Some(chrono::Utc::now());
                }
            }
            for wt in m.worktrees.values_mut() {
                for agent in wt.agents.values_mut() {
                    if !agent.suspended
                        && matches!(agent.status, AgentStatus::Streaming | AgentStatus::Waiting)
                        && agent.pid.is_none_or(|pid| !Self::is_pid_alive(pid))
                    {
                        agent.status = AgentStatus::Broken;
                        agent.completed_at = Some(chrono::Utc::now());
                    }
                }
            }
            m
        })
        .ok();
    }

    async fn rollback_worktree(
        &self,
        root_path: &Path,
        worktree_id: Option<&str>,
        branch: Option<&str>,
    ) {
        if let Some(wt_id) = worktree_id {
            let wt_path = paths::worktree_path(root_path, wt_id);
            git::remove_worktree(root_path, &wt_path).await.ok();
        }
        if let Some(b) = branch {
            git::delete_local_branch(root_path, b).await.ok();
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

    /// Read manifest from disk (off async runtime).
    async fn read_manifest_async(&self, project_root: &str) -> Result<Manifest, PuError> {
        let pr = project_root.to_string();
        tokio::task::spawn_blocking(move || manifest::read_manifest(Path::new(&pr)))
            .await
            .unwrap_or_else(|e| Err(PuError::Io(std::io::Error::other(e))))
    }

    // --- Status Push ---

    async fn handle_subscribe_status(&self, project_root: &str) -> Response {
        let mut channels = self.status_channels.lock().await;
        channels
            .entry(project_root.to_string())
            .or_insert_with(|| tokio::sync::broadcast::channel(64).0);
        Response::StatusSubscribed
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
    async fn notify_status_change(&self, project_root: &str) {
        let channels = self.status_channels.lock().await;
        if let Some(tx) = channels.get(project_root) {
            let _ = tx.send(());
        }
    }

    /// Compute a full status report for a project (used by status push and handle_status).
    pub async fn compute_full_status(
        &self,
        project_root: &str,
    ) -> Option<(Vec<WorktreeEntry>, Vec<AgentStatusReport>)> {
        let m = self.read_manifest_async(project_root).await.ok()?;
        let sessions = self.sessions.lock().await;
        let mut agents: Vec<AgentStatusReport> = m
            .agents
            .values()
            .map(|a| {
                let (status, exit_code, idle_seconds) =
                    self.live_agent_status_sync(&a.id, a, &sessions);
                AgentStatusReport {
                    id: a.id.clone(),
                    name: a.name.clone(),
                    agent_type: a.agent_type.clone(),
                    status,
                    pid: a.pid,
                    exit_code,
                    idle_seconds,
                    worktree_id: None,
                    started_at: a.started_at,
                    session_id: a.session_id.clone(),
                    prompt: a.prompt.clone(),
                    suspended: a.suspended,
                }
            })
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
        Some((worktrees, agents))
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        // Kill all child processes so spawn_blocking reader/waitpid tasks can finish.
        if let Ok(sessions) = self.sessions.try_lock() {
            for handle in sessions.values() {
                unsafe {
                    libc::kill(handle.pid as i32, libc::SIGKILL);
                }
            }
        }
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
        assert!(
            handles.is_some(),
            "expected attach handles for spawned agent"
        );

        let (buffer, _fd, _exit_rx) = handles.unwrap();
        // Buffer exists and has a valid offset
        let _ = buffer.current_offset();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn given_unknown_agent_should_return_none() {
        let engine = Engine::new();
        let handles = engine.get_attach_handles("ag-nonexistent").await;
        assert!(handles.is_none());
    }
}
