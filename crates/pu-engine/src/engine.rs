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
    AgentDefInfo, AgentStatusReport, GridCommand, KillTarget, PROTOCOL_VERSION, Request, Response,
    ScheduleInfo, ScheduleTriggerPayload, SuspendTarget, SwarmDefInfo, SwarmRosterEntryPayload,
    TemplateInfo,
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
    pending_initial_inputs: Arc<Mutex<HashMap<String, Vec<u8>>>>,
    login_env: Arc<OnceCell<Vec<(String, String)>>>,
    reaped_projects: Arc<std::sync::Mutex<HashSet<String>>>,
    /// Per-project broadcast channels for grid commands.
    grid_channels: Arc<Mutex<HashMap<String, tokio::sync::broadcast::Sender<GridCommand>>>>,
    /// Per-project broadcast channels for status push updates.
    status_channels: Arc<Mutex<HashMap<String, tokio::sync::broadcast::Sender<()>>>>,
    /// Projects that have been initialized or used — scheduler scans these.
    registered_projects: Arc<std::sync::Mutex<HashSet<String>>>,
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
            pending_initial_inputs: Arc::new(Mutex::new(HashMap::new())),
            login_env: Arc::new(OnceCell::new()),
            reaped_projects: Arc::new(std::sync::Mutex::new(HashSet::new())),
            grid_channels: Arc::new(Mutex::new(HashMap::new())),
            status_channels: Arc::new(Mutex::new(HashMap::new())),
            registered_projects: Arc::new(std::sync::Mutex::new(HashSet::new())),
        }
    }

    /// Start a background task that periodically removes session handles for
    /// processes that have exited naturally, and cleans up broadcast channels
    /// with no subscribers. Without this, HashMap entries leak.
    pub fn start_session_reaper(self: &Arc<Self>) {
        let sessions = self.sessions.clone();
        let pending_initial_inputs = self.pending_initial_inputs.clone();
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
                    let mut pending_initial_inputs = pending_initial_inputs.lock().await;
                    for id in &dead_ids {
                        pending_initial_inputs.remove(id);
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

    async fn resolve_login_env() -> Vec<(String, String)> {
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into());
        match tokio::process::Command::new(&shell)
            .args(["-li", "-c", "env -0"])
            .stdin(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .output()
            .await
        {
            Ok(output) if output.status.success() => output
                .stdout
                .split(|&b| b == 0)
                .filter_map(|entry| {
                    let s = std::str::from_utf8(entry).ok()?;
                    let (k, v) = s.split_once('=')?;
                    if k.is_empty() {
                        return None;
                    }
                    Some((k.to_string(), v.to_string()))
                })
                .collect(),
            // Fallback: use the daemon's own env
            _ => std::env::vars().collect(),
        }
    }

    fn register_project(&self, project_root: &str) {
        if !project_root.is_empty() {
            if let Ok(mut projects) = self.registered_projects.lock() {
                projects.insert(project_root.to_string());
            }
        }
    }

    pub fn registered_projects(&self) -> Vec<String> {
        self.registered_projects
            .lock()
            .map(|p| p.iter().cloned().collect())
            .unwrap_or_default()
    }

    pub async fn handle_request(&self, request: Request) -> Response {
        // Register project for any project-scoped request
        match &request {
            Request::Init { project_root }
            | Request::Spawn { project_root, .. }
            | Request::Status { project_root, .. }
            | Request::Kill { project_root, .. }
            | Request::ListTemplates { project_root }
            | Request::ListAgentDefs { project_root }
            | Request::ListSwarmDefs { project_root }
            | Request::ListSchedules { project_root }
            | Request::SaveSchedule { project_root, .. }
            | Request::EnableSchedule { project_root, .. }
            | Request::DisableSchedule { project_root, .. } => {
                self.register_project(project_root);
            }
            _ => {}
        }

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
            // Template CRUD
            Request::ListTemplates { project_root } => {
                self.handle_list_templates(&project_root).await
            }
            Request::GetTemplate { project_root, name } => {
                self.handle_get_template(&project_root, &name).await
            }
            Request::SaveTemplate {
                project_root,
                name,
                description,
                agent,
                body,
                scope,
            } => {
                self.handle_save_template(&project_root, &name, &description, &agent, &body, &scope)
                    .await
            }
            Request::DeleteTemplate {
                project_root,
                name,
                scope,
            } => {
                self.handle_delete_template(&project_root, &name, &scope)
                    .await
            }
            // Agent def CRUD
            Request::ListAgentDefs { project_root } => {
                self.handle_list_agent_defs(&project_root).await
            }
            Request::GetAgentDef { project_root, name } => {
                self.handle_get_agent_def(&project_root, &name).await
            }
            Request::SaveAgentDef {
                project_root,
                name,
                agent_type,
                template,
                inline_prompt,
                tags,
                scope,
                available_in_command_dialog,
                icon,
            } => {
                self.handle_save_agent_def(
                    &project_root,
                    &name,
                    &agent_type,
                    template,
                    inline_prompt,
                    tags,
                    &scope,
                    available_in_command_dialog,
                    icon,
                )
                .await
            }
            Request::DeleteAgentDef {
                project_root,
                name,
                scope,
            } => {
                self.handle_delete_agent_def(&project_root, &name, &scope)
                    .await
            }
            // Swarm def CRUD
            Request::ListSwarmDefs { project_root } => {
                self.handle_list_swarm_defs(&project_root).await
            }
            Request::GetSwarmDef { project_root, name } => {
                self.handle_get_swarm_def(&project_root, &name).await
            }
            Request::SaveSwarmDef {
                project_root,
                name,
                worktree_count,
                worktree_template,
                roster,
                include_terminal,
                scope,
            } => {
                self.handle_save_swarm_def(
                    &project_root,
                    &name,
                    worktree_count,
                    &worktree_template,
                    roster,
                    include_terminal,
                    &scope,
                )
                .await
            }
            Request::DeleteSwarmDef {
                project_root,
                name,
                scope,
            } => {
                self.handle_delete_swarm_def(&project_root, &name, &scope)
                    .await
            }
            // Execution
            Request::RunSwarm {
                project_root,
                swarm_name,
                vars,
            } => {
                self.handle_run_swarm(&project_root, &swarm_name, vars)
                    .await
            }
            // Schedule CRUD
            Request::ListSchedules { project_root } => {
                self.handle_list_schedules(&project_root).await
            }
            Request::GetSchedule { project_root, name } => {
                self.handle_get_schedule(&project_root, &name).await
            }
            Request::SaveSchedule {
                project_root,
                name,
                enabled,
                recurrence,
                start_at,
                trigger,
                target,
                scope,
            } => {
                self.handle_save_schedule(
                    &project_root,
                    &name,
                    enabled,
                    &recurrence,
                    start_at,
                    trigger,
                    &target,
                    &scope,
                )
                .await
            }
            Request::DeleteSchedule {
                project_root,
                name,
                scope,
            } => {
                self.handle_delete_schedule(&project_root, &name, &scope)
                    .await
            }
            Request::EnableSchedule { project_root, name } => {
                self.handle_enable_schedule(&project_root, &name).await
            }
            Request::DisableSchedule { project_root, name } => {
                self.handle_disable_schedule(&project_root, &name).await
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

        // Claude prompt via argv can stall first render in some terminals; keep stdin injection
        // for Claude (and terminal agent). Codex/OpenCode accept startup prompts via CLI args.
        let inject_prompt_via_stdin =
            Self::should_inject_prompt_via_stdin(agent_type, agent_cfg.interactive, prompt);
        if !inject_prompt_via_stdin && !prompt.is_empty() {
            let prompt_flag =
                Self::resolved_prompt_flag(agent_type, agent_cfg.prompt_flag.as_deref());
            if let Some(flag) = prompt_flag {
                args.push(flag);
                args.push(prompt.to_string());
            } else {
                // Default prompt style is positional (for example codex [PROMPT]).
                args.push(prompt.to_string());
            }
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
            env: self.agent_env().await,
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

        if inject_prompt_via_stdin {
            let input = prompt.as_bytes().to_vec();
            self.pending_initial_inputs
                .lock()
                .await
                .insert(agent_id.clone(), input);
        }

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

        if let Err(e) = manifest_result {
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
        {
            let mut pending_inputs = self.pending_initial_inputs.lock().await;
            for id in &agent_ids {
                pending_inputs.remove(id);
            }
        }
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
        {
            let mut pending_inputs = self.pending_initial_inputs.lock().await;
            for id in &agent_ids {
                pending_inputs.remove(id);
            }
        }
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

        {
            let mut pending_inputs = self.pending_initial_inputs.lock().await;
            for id in &agent_ids {
                pending_inputs.remove(id);
            }
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
            env: self.agent_env().await,
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

    /// Take and clear any queued initial input for an agent.
    pub async fn take_initial_input(&self, agent_id: &str) -> Option<Vec<u8>> {
        self.pending_initial_inputs.lock().await.remove(agent_id)
    }

    /// Re-queue initial input if an attach session ended before delivery.
    pub async fn restore_initial_input(&self, agent_id: &str, data: Vec<u8>) {
        self.pending_initial_inputs
            .lock()
            .await
            .entry(agent_id.to_string())
            .or_insert(data);
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

    /// Build the full environment for spawned agents.
    /// Starts from the user's login shell env, then overrides PATH
    /// (prepends ~/.pu/bin + fallback dirs), TERM, and COLORTERM.
    async fn agent_env(&self) -> Vec<(String, String)> {
        let login_env = self.login_env.get_or_init(Self::resolve_login_env).await;
        let mut env = login_env.clone();

        // Extract login PATH for augmentation
        let login_path = env
            .iter()
            .find(|(k, _)| k == "PATH")
            .map(|(_, v)| v.clone())
            .unwrap_or_default();

        // Append common fallback dirs (guards against missing-binary issues)
        let home = std::env::var("HOME").unwrap_or_default();
        let fallbacks = [
            format!("{home}/.local/bin"),
            format!("{home}/.cargo/bin"),
            "/usr/local/bin".to_string(),
            "/opt/homebrew/bin".to_string(),
        ];
        let mut path = login_path;
        for dir in fallbacks {
            if !path.split(':').any(|p| p == dir) {
                path = format!("{path}:{dir}");
            }
        }
        // Prepend ~/.pu/bin
        if let Ok(pu_dir) = paths::global_pu_dir() {
            path = format!("{}:{}", pu_dir.join("bin").display(), path);
        }

        // Override PATH, TERM, COLORTERM in the env
        env.retain(|(k, _)| k != "PATH" && k != "TERM" && k != "COLORTERM");
        env.push(("PATH".into(), path));
        env.push(("TERM".into(), "xterm-256color".into()));
        env.push(("COLORTERM".into(), "truecolor".into()));

        env
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

    fn should_inject_prompt_via_stdin(agent_type: &str, interactive: bool, prompt: &str) -> bool {
        !prompt.is_empty() && interactive && matches!(agent_type, "claude" | "terminal")
    }

    fn resolved_prompt_flag(agent_type: &str, prompt_flag: Option<&str>) -> Option<String> {
        match (agent_type, prompt_flag) {
            ("opencode", None) => Some("--prompt".to_string()),
            (_, Some(flag)) => Some(flag.to_string()),
            _ => None,
        }
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

    // --- Template CRUD handlers ---

    async fn handle_list_templates(&self, project_root: &str) -> Response {
        let pr = project_root.to_string();
        match tokio::task::spawn_blocking(move || {
            let root = Path::new(&pr);
            let templates = pu_core::template::list_templates(root);
            let infos: Vec<TemplateInfo> = templates
                .into_iter()
                .map(|t| TemplateInfo {
                    name: t.name,
                    description: t.description,
                    agent: t.agent,
                    source: t.source,
                    variables: pu_core::template::extract_variables(&t.body),
                })
                .collect();
            infos
        })
        .await
        {
            Ok(templates) => Response::TemplateList { templates },
            Err(e) => Response::Error {
                code: "INTERNAL_ERROR".into(),
                message: format!("task join error: {e}"),
            },
        }
    }

    async fn handle_get_template(&self, project_root: &str, name: &str) -> Response {
        let pr = project_root.to_string();
        let tpl_name = name.to_string();
        match tokio::task::spawn_blocking(move || {
            let root = Path::new(&pr);
            pu_core::template::find_template(root, &tpl_name)
        })
        .await
        {
            Ok(Some(t)) => Response::TemplateDetail {
                name: t.name,
                description: t.description,
                agent: t.agent,
                variables: pu_core::template::extract_variables(&t.body),
                body: t.body,
                source: t.source,
            },
            Ok(None) => Response::Error {
                code: "NOT_FOUND".into(),
                message: format!("template '{name}' not found"),
            },
            Err(e) => Response::Error {
                code: "INTERNAL_ERROR".into(),
                message: format!("task join error: {e}"),
            },
        }
    }

    async fn handle_save_template(
        &self,
        project_root: &str,
        name: &str,
        description: &str,
        agent: &str,
        body: &str,
        scope: &str,
    ) -> Response {
        let dir = match Self::resolve_scope_dir(
            project_root,
            scope,
            paths::templates_dir,
            paths::global_templates_dir,
        ) {
            Ok(d) => d,
            Err(msg) => {
                return Response::Error {
                    code: "IO_ERROR".into(),
                    message: msg,
                };
            }
        };
        let n = name.to_string();
        let d = description.to_string();
        let a = agent.to_string();
        let b = body.to_string();
        match tokio::task::spawn_blocking(move || {
            pu_core::template::save_template(&dir, &n, &d, &a, &b)
        })
        .await
        {
            Ok(Ok(())) => Response::Ok,
            Ok(Err(e)) => Response::Error {
                code: "IO_ERROR".into(),
                message: format!("failed to save template: {e}"),
            },
            Err(e) => Response::Error {
                code: "INTERNAL_ERROR".into(),
                message: format!("task join error: {e}"),
            },
        }
    }

    async fn handle_delete_template(
        &self,
        project_root: &str,
        name: &str,
        scope: &str,
    ) -> Response {
        let dir = match Self::resolve_scope_dir(
            project_root,
            scope,
            paths::templates_dir,
            paths::global_templates_dir,
        ) {
            Ok(d) => d,
            Err(msg) => {
                return Response::Error {
                    code: "IO_ERROR".into(),
                    message: msg,
                };
            }
        };
        let n = name.to_string();
        match tokio::task::spawn_blocking(move || pu_core::template::delete_template(&dir, &n))
            .await
        {
            Ok(Ok(_)) => Response::Ok,
            Ok(Err(e)) => Response::Error {
                code: "IO_ERROR".into(),
                message: format!("failed to delete template: {e}"),
            },
            Err(e) => Response::Error {
                code: "INTERNAL_ERROR".into(),
                message: format!("task join error: {e}"),
            },
        }
    }

    // --- Agent def CRUD handlers ---

    async fn handle_list_agent_defs(&self, project_root: &str) -> Response {
        let pr = project_root.to_string();
        match tokio::task::spawn_blocking(move || {
            let root = Path::new(&pr);
            let defs = pu_core::agent_def::list_agent_defs(root);
            let infos: Vec<AgentDefInfo> = defs
                .into_iter()
                .map(|d| AgentDefInfo {
                    name: d.name,
                    agent_type: d.agent_type,
                    template: d.template,
                    inline_prompt: d.inline_prompt,
                    tags: d.tags,
                    scope: d.scope,
                    available_in_command_dialog: d.available_in_command_dialog,
                    icon: d.icon,
                })
                .collect();
            infos
        })
        .await
        {
            Ok(agent_defs) => Response::AgentDefList { agent_defs },
            Err(e) => Response::Error {
                code: "INTERNAL_ERROR".into(),
                message: format!("task join error: {e}"),
            },
        }
    }

    async fn handle_get_agent_def(&self, project_root: &str, name: &str) -> Response {
        let pr = project_root.to_string();
        let n = name.to_string();
        match tokio::task::spawn_blocking(move || {
            pu_core::agent_def::find_agent_def(Path::new(&pr), &n)
        })
        .await
        {
            Ok(Some(d)) => Response::AgentDefDetail {
                name: d.name,
                agent_type: d.agent_type,
                template: d.template,
                inline_prompt: d.inline_prompt,
                tags: d.tags,
                scope: d.scope,
                available_in_command_dialog: d.available_in_command_dialog,
                icon: d.icon,
            },
            Ok(None) => Response::Error {
                code: "NOT_FOUND".into(),
                message: format!("agent def '{name}' not found"),
            },
            Err(e) => Response::Error {
                code: "INTERNAL_ERROR".into(),
                message: format!("task join error: {e}"),
            },
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn handle_save_agent_def(
        &self,
        project_root: &str,
        name: &str,
        agent_type: &str,
        template: Option<String>,
        inline_prompt: Option<String>,
        tags: Vec<String>,
        scope: &str,
        available_in_command_dialog: bool,
        icon: Option<String>,
    ) -> Response {
        let dir = match Self::resolve_scope_dir(
            project_root,
            scope,
            paths::agents_dir,
            paths::global_agents_dir,
        ) {
            Ok(d) => d,
            Err(msg) => {
                return Response::Error {
                    code: "IO_ERROR".into(),
                    message: msg,
                };
            }
        };
        let def = pu_core::agent_def::AgentDef {
            name: name.to_string(),
            agent_type: agent_type.to_string(),
            template,
            inline_prompt,
            tags,
            scope: scope.to_string(),
            available_in_command_dialog,
            icon,
        };
        match tokio::task::spawn_blocking(move || pu_core::agent_def::save_agent_def(&dir, &def))
            .await
        {
            Ok(Ok(())) => Response::Ok,
            Ok(Err(e)) => Response::Error {
                code: "IO_ERROR".into(),
                message: format!("failed to save agent def: {e}"),
            },
            Err(e) => Response::Error {
                code: "INTERNAL_ERROR".into(),
                message: format!("task join error: {e}"),
            },
        }
    }

    async fn handle_delete_agent_def(
        &self,
        project_root: &str,
        name: &str,
        scope: &str,
    ) -> Response {
        let dir = match Self::resolve_scope_dir(
            project_root,
            scope,
            paths::agents_dir,
            paths::global_agents_dir,
        ) {
            Ok(d) => d,
            Err(msg) => {
                return Response::Error {
                    code: "IO_ERROR".into(),
                    message: msg,
                };
            }
        };
        let n = name.to_string();
        match tokio::task::spawn_blocking(move || pu_core::agent_def::delete_agent_def(&dir, &n))
            .await
        {
            Ok(Ok(_)) => Response::Ok,
            Ok(Err(e)) => Response::Error {
                code: "IO_ERROR".into(),
                message: format!("failed to delete agent def: {e}"),
            },
            Err(e) => Response::Error {
                code: "INTERNAL_ERROR".into(),
                message: format!("task join error: {e}"),
            },
        }
    }

    // --- Swarm def CRUD handlers ---

    async fn handle_list_swarm_defs(&self, project_root: &str) -> Response {
        let pr = project_root.to_string();
        match tokio::task::spawn_blocking(move || {
            let root = Path::new(&pr);
            let defs = pu_core::swarm_def::list_swarm_defs(root);
            let infos: Vec<SwarmDefInfo> = defs
                .into_iter()
                .map(|d| SwarmDefInfo {
                    name: d.name,
                    worktree_count: d.worktree_count,
                    worktree_template: d.worktree_template,
                    roster: d
                        .roster
                        .into_iter()
                        .map(|r| SwarmRosterEntryPayload {
                            agent_def: r.agent_def,
                            role: r.role,
                            quantity: r.quantity,
                        })
                        .collect(),
                    include_terminal: d.include_terminal,
                    scope: d.scope,
                })
                .collect();
            infos
        })
        .await
        {
            Ok(swarm_defs) => Response::SwarmDefList { swarm_defs },
            Err(e) => Response::Error {
                code: "INTERNAL_ERROR".into(),
                message: format!("task join error: {e}"),
            },
        }
    }

    async fn handle_get_swarm_def(&self, project_root: &str, name: &str) -> Response {
        let pr = project_root.to_string();
        let n = name.to_string();
        match tokio::task::spawn_blocking(move || {
            pu_core::swarm_def::find_swarm_def(Path::new(&pr), &n)
        })
        .await
        {
            Ok(Some(d)) => Response::SwarmDefDetail {
                name: d.name,
                worktree_count: d.worktree_count,
                worktree_template: d.worktree_template,
                roster: d
                    .roster
                    .into_iter()
                    .map(|r| SwarmRosterEntryPayload {
                        agent_def: r.agent_def,
                        role: r.role,
                        quantity: r.quantity,
                    })
                    .collect(),
                include_terminal: d.include_terminal,
                scope: d.scope,
            },
            Ok(None) => Response::Error {
                code: "NOT_FOUND".into(),
                message: format!("swarm def '{name}' not found"),
            },
            Err(e) => Response::Error {
                code: "INTERNAL_ERROR".into(),
                message: format!("task join error: {e}"),
            },
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn handle_save_swarm_def(
        &self,
        project_root: &str,
        name: &str,
        worktree_count: u32,
        worktree_template: &str,
        roster: Vec<SwarmRosterEntryPayload>,
        include_terminal: bool,
        scope: &str,
    ) -> Response {
        let dir = match Self::resolve_scope_dir(
            project_root,
            scope,
            paths::swarms_dir,
            paths::global_swarms_dir,
        ) {
            Ok(d) => d,
            Err(msg) => {
                return Response::Error {
                    code: "IO_ERROR".into(),
                    message: msg,
                };
            }
        };
        let def = pu_core::swarm_def::SwarmDef {
            name: name.to_string(),
            worktree_count,
            worktree_template: worktree_template.to_string(),
            roster: roster
                .into_iter()
                .map(|r| pu_core::swarm_def::SwarmRosterEntry {
                    agent_def: r.agent_def,
                    role: r.role,
                    quantity: r.quantity,
                })
                .collect(),
            include_terminal,
            scope: scope.to_string(),
        };
        match tokio::task::spawn_blocking(move || pu_core::swarm_def::save_swarm_def(&dir, &def))
            .await
        {
            Ok(Ok(())) => Response::Ok,
            Ok(Err(e)) => Response::Error {
                code: "IO_ERROR".into(),
                message: format!("failed to save swarm def: {e}"),
            },
            Err(e) => Response::Error {
                code: "INTERNAL_ERROR".into(),
                message: format!("task join error: {e}"),
            },
        }
    }

    async fn handle_delete_swarm_def(
        &self,
        project_root: &str,
        name: &str,
        scope: &str,
    ) -> Response {
        let dir = match Self::resolve_scope_dir(
            project_root,
            scope,
            paths::swarms_dir,
            paths::global_swarms_dir,
        ) {
            Ok(d) => d,
            Err(msg) => {
                return Response::Error {
                    code: "IO_ERROR".into(),
                    message: msg,
                };
            }
        };
        let n = name.to_string();
        match tokio::task::spawn_blocking(move || pu_core::swarm_def::delete_swarm_def(&dir, &n))
            .await
        {
            Ok(Ok(_)) => Response::Ok,
            Ok(Err(e)) => Response::Error {
                code: "IO_ERROR".into(),
                message: format!("failed to delete swarm def: {e}"),
            },
            Err(e) => Response::Error {
                code: "INTERNAL_ERROR".into(),
                message: format!("task join error: {e}"),
            },
        }
    }

    // --- RunSwarm handler ---

    async fn handle_run_swarm(
        &self,
        project_root: &str,
        swarm_name: &str,
        vars: std::collections::HashMap<String, String>,
    ) -> Response {
        // Read the swarm definition
        let pr = project_root.to_string();
        let sn = swarm_name.to_string();
        let swarm_def = match tokio::task::spawn_blocking(move || {
            pu_core::swarm_def::find_swarm_def(Path::new(&pr), &sn)
        })
        .await
        {
            Ok(Some(def)) => def,
            Ok(None) => {
                return Response::Error {
                    code: "NOT_FOUND".into(),
                    message: format!("swarm def '{swarm_name}' not found"),
                };
            }
            Err(e) => {
                return Response::Error {
                    code: "INTERNAL_ERROR".into(),
                    message: format!("task join error: {e}"),
                };
            }
        };

        let mut spawned_agents = Vec::new();

        for wt_index in 0..swarm_def.worktree_count {
            let wt_name = if swarm_def.worktree_template.is_empty() {
                format!("{swarm_name}-{wt_index}")
            } else {
                swarm_def
                    .worktree_template
                    .replace("{index}", &wt_index.to_string())
            };

            let mut worktree_id: Option<String> = None;

            for entry in &swarm_def.roster {
                // Resolve agent def to get template/inline_prompt
                let pr2 = project_root.to_string();
                let ad_name = entry.agent_def.clone();
                let agent_def = match tokio::task::spawn_blocking(move || {
                    pu_core::agent_def::find_agent_def(Path::new(&pr2), &ad_name)
                })
                .await
                {
                    Ok(Some(def)) => def,
                    Ok(None) => {
                        return Response::Error {
                            code: "NOT_FOUND".into(),
                            message: format!(
                                "agent def '{}' referenced by swarm not found",
                                entry.agent_def
                            ),
                        };
                    }
                    Err(e) => {
                        return Response::Error {
                            code: "INTERNAL_ERROR".into(),
                            message: format!("task join error: {e}"),
                        };
                    }
                };

                // Resolve prompt: template or inline
                let prompt = if let Some(ref tpl_name) = agent_def.template {
                    let pr3 = project_root.to_string();
                    let tn = tpl_name.clone();
                    let vars_clone = vars.clone();
                    match tokio::task::spawn_blocking(move || {
                        pu_core::template::find_template(Path::new(&pr3), &tn)
                    })
                    .await
                    {
                        Ok(Some(tpl)) => pu_core::template::render(&tpl, &vars_clone),
                        Ok(None) => {
                            return Response::Error {
                                code: "NOT_FOUND".into(),
                                message: format!("template '{tpl_name}' not found"),
                            };
                        }
                        Err(e) => {
                            return Response::Error {
                                code: "INTERNAL_ERROR".into(),
                                message: format!("task join error: {e}"),
                            };
                        }
                    }
                } else {
                    agent_def.inline_prompt.clone().unwrap_or_default()
                };

                for q in 0..entry.quantity {
                    let agent_name = format!("{}-{}-{wt_index}-{q}", swarm_name, entry.agent_def);

                    // First agent creates the worktree; subsequent agents reuse it
                    let (spawn_name, spawn_worktree) = if worktree_id.is_some() {
                        (Some(agent_name), worktree_id.clone())
                    } else {
                        (Some(wt_name.clone()), None)
                    };

                    let resp = self
                        .handle_spawn(
                            project_root,
                            &prompt,
                            &agent_def.agent_type,
                            spawn_name,
                            None,
                            false,
                            spawn_worktree,
                        )
                        .await;

                    match resp {
                        Response::SpawnResult {
                            agent_id,
                            worktree_id: wt_id,
                            ..
                        } => {
                            spawned_agents.push(agent_id);
                            if worktree_id.is_none() {
                                worktree_id = wt_id;
                            }
                        }
                        Response::Error { code, message } => {
                            return Response::Error { code, message };
                        }
                        _ => {}
                    }
                }
            }
        }

        Response::RunSwarmResult { spawned_agents }
    }

    // --- Schedule handlers ---

    async fn handle_list_schedules(&self, project_root: &str) -> Response {
        let pr = project_root.to_string();
        match tokio::task::spawn_blocking(move || {
            let root = Path::new(&pr);
            let defs = pu_core::schedule_def::list_schedule_defs(root);
            let infos: Vec<ScheduleInfo> = defs
                .into_iter()
                .map(Self::schedule_def_to_info)
                .collect();
            infos
        })
        .await
        {
            Ok(schedules) => Response::ScheduleList { schedules },
            Err(e) => Response::Error {
                code: "INTERNAL_ERROR".into(),
                message: format!("task join error: {e}"),
            },
        }
    }

    async fn handle_get_schedule(&self, project_root: &str, name: &str) -> Response {
        let pr = project_root.to_string();
        let n = name.to_string();
        match tokio::task::spawn_blocking(move || {
            pu_core::schedule_def::find_schedule_def(Path::new(&pr), &n)
        })
        .await
        {
            Ok(Some(d)) => Self::schedule_def_to_detail(d),
            Ok(None) => Response::Error {
                code: "NOT_FOUND".into(),
                message: format!("schedule '{name}' not found"),
            },
            Err(e) => Response::Error {
                code: "INTERNAL_ERROR".into(),
                message: format!("task join error: {e}"),
            },
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn handle_save_schedule(
        &self,
        project_root: &str,
        name: &str,
        enabled: bool,
        recurrence: &str,
        start_at: chrono::DateTime<chrono::Utc>,
        trigger: ScheduleTriggerPayload,
        target: &str,
        scope: &str,
    ) -> Response {
        let dir = match Self::resolve_scope_dir(
            project_root,
            scope,
            paths::schedules_dir,
            paths::global_schedules_dir,
        ) {
            Ok(d) => d,
            Err(msg) => {
                return Response::Error {
                    code: "IO_ERROR".into(),
                    message: msg,
                };
            }
        };
        let rec = match Self::parse_recurrence(recurrence) {
            Ok(r) => r,
            Err(msg) => {
                return Response::Error {
                    code: "INVALID_INPUT".into(),
                    message: msg,
                };
            }
        };
        let now = chrono::Utc::now();
        let next_run = if enabled {
            pu_core::schedule_def::next_occurrence(start_at, &rec, now)
        } else {
            None
        };
        let def = pu_core::schedule_def::ScheduleDef {
            name: name.to_string(),
            enabled,
            recurrence: rec,
            start_at,
            next_run,
            trigger: Self::payload_to_trigger(&trigger),
            project_root: project_root.to_string(),
            target: target.to_string(),
            scope: scope.to_string(),
            created_at: now,
        };
        match tokio::task::spawn_blocking(move || {
            pu_core::schedule_def::save_schedule_def(&dir, &def)
        })
        .await
        {
            Ok(Ok(())) => Response::Ok,
            Ok(Err(e)) => Response::Error {
                code: "IO_ERROR".into(),
                message: format!("failed to save schedule: {e}"),
            },
            Err(e) => Response::Error {
                code: "INTERNAL_ERROR".into(),
                message: format!("task join error: {e}"),
            },
        }
    }

    async fn handle_delete_schedule(
        &self,
        project_root: &str,
        name: &str,
        scope: &str,
    ) -> Response {
        let dir = match Self::resolve_scope_dir(
            project_root,
            scope,
            paths::schedules_dir,
            paths::global_schedules_dir,
        ) {
            Ok(d) => d,
            Err(msg) => {
                return Response::Error {
                    code: "IO_ERROR".into(),
                    message: msg,
                };
            }
        };
        let n = name.to_string();
        match tokio::task::spawn_blocking(move || {
            pu_core::schedule_def::delete_schedule_def(&dir, &n)
        })
        .await
        {
            Ok(Ok(_)) => Response::Ok,
            Ok(Err(e)) => Response::Error {
                code: "IO_ERROR".into(),
                message: format!("failed to delete schedule: {e}"),
            },
            Err(e) => Response::Error {
                code: "INTERNAL_ERROR".into(),
                message: format!("task join error: {e}"),
            },
        }
    }

    async fn handle_enable_schedule(&self, project_root: &str, name: &str) -> Response {
        let pr = project_root.to_string();
        let n = name.to_string();
        tokio::task::spawn_blocking(move || {
            let root = Path::new(&pr);
            let mut def = match pu_core::schedule_def::find_schedule_def(root, &n) {
                Some(d) => d,
                None => {
                    return Response::Error {
                        code: "NOT_FOUND".into(),
                        message: format!("schedule '{n}' not found"),
                    };
                }
            };
            def.enabled = true;
            let now = chrono::Utc::now();
            def.next_run =
                pu_core::schedule_def::next_occurrence(def.start_at, &def.recurrence, now);
            let dir = paths::schedules_dir(root);
            match pu_core::schedule_def::save_schedule_def(&dir, &def) {
                Ok(()) => Response::Ok,
                Err(e) => Response::Error {
                    code: "IO_ERROR".into(),
                    message: format!("failed to save schedule: {e}"),
                },
            }
        })
        .await
        .unwrap_or_else(|e| Response::Error {
            code: "INTERNAL_ERROR".into(),
            message: format!("task join error: {e}"),
        })
    }

    async fn handle_disable_schedule(&self, project_root: &str, name: &str) -> Response {
        let pr = project_root.to_string();
        let n = name.to_string();
        tokio::task::spawn_blocking(move || {
            let root = Path::new(&pr);
            let mut def = match pu_core::schedule_def::find_schedule_def(root, &n) {
                Some(d) => d,
                None => {
                    return Response::Error {
                        code: "NOT_FOUND".into(),
                        message: format!("schedule '{n}' not found"),
                    };
                }
            };
            def.enabled = false;
            def.next_run = None;
            let dir = paths::schedules_dir(root);
            match pu_core::schedule_def::save_schedule_def(&dir, &def) {
                Ok(()) => Response::Ok,
                Err(e) => Response::Error {
                    code: "IO_ERROR".into(),
                    message: format!("failed to save schedule: {e}"),
                },
            }
        })
        .await
        .unwrap_or_else(|e| Response::Error {
            code: "INTERNAL_ERROR".into(),
            message: format!("task join error: {e}"),
        })
    }

    fn schedule_def_to_info(d: pu_core::schedule_def::ScheduleDef) -> ScheduleInfo {
        ScheduleInfo {
            name: d.name,
            enabled: d.enabled,
            recurrence: Self::recurrence_to_string(&d.recurrence),
            start_at: d.start_at,
            next_run: d.next_run,
            trigger: Self::trigger_to_payload(&d.trigger),
            project_root: d.project_root,
            target: d.target,
            scope: d.scope,
            created_at: d.created_at,
        }
    }

    fn schedule_def_to_detail(d: pu_core::schedule_def::ScheduleDef) -> Response {
        Response::ScheduleDetail {
            name: d.name,
            enabled: d.enabled,
            recurrence: Self::recurrence_to_string(&d.recurrence),
            start_at: d.start_at,
            next_run: d.next_run,
            trigger: Self::trigger_to_payload(&d.trigger),
            project_root: d.project_root,
            target: d.target,
            scope: d.scope,
            created_at: d.created_at,
        }
    }

    fn recurrence_to_string(r: &pu_core::schedule_def::Recurrence) -> String {
        match r {
            pu_core::schedule_def::Recurrence::None => "none",
            pu_core::schedule_def::Recurrence::Hourly => "hourly",
            pu_core::schedule_def::Recurrence::Daily => "daily",
            pu_core::schedule_def::Recurrence::Weekdays => "weekdays",
            pu_core::schedule_def::Recurrence::Weekly => "weekly",
            pu_core::schedule_def::Recurrence::Monthly => "monthly",
        }
        .to_string()
    }

    fn parse_recurrence(s: &str) -> Result<pu_core::schedule_def::Recurrence, String> {
        match s {
            "none" => Ok(pu_core::schedule_def::Recurrence::None),
            "hourly" => Ok(pu_core::schedule_def::Recurrence::Hourly),
            "daily" => Ok(pu_core::schedule_def::Recurrence::Daily),
            "weekdays" => Ok(pu_core::schedule_def::Recurrence::Weekdays),
            "weekly" => Ok(pu_core::schedule_def::Recurrence::Weekly),
            "monthly" => Ok(pu_core::schedule_def::Recurrence::Monthly),
            other => Err(format!("unknown recurrence: {other}")),
        }
    }

    fn trigger_to_payload(t: &pu_core::schedule_def::ScheduleTrigger) -> ScheduleTriggerPayload {
        match t {
            pu_core::schedule_def::ScheduleTrigger::AgentDef { name } => {
                ScheduleTriggerPayload::AgentDef { name: name.clone() }
            }
            pu_core::schedule_def::ScheduleTrigger::SwarmDef { name, vars } => {
                ScheduleTriggerPayload::SwarmDef {
                    name: name.clone(),
                    vars: vars.clone(),
                }
            }
            pu_core::schedule_def::ScheduleTrigger::InlinePrompt { prompt, agent } => {
                ScheduleTriggerPayload::InlinePrompt {
                    prompt: prompt.clone(),
                    agent: agent.clone(),
                }
            }
        }
    }

    fn payload_to_trigger(p: &ScheduleTriggerPayload) -> pu_core::schedule_def::ScheduleTrigger {
        match p {
            ScheduleTriggerPayload::AgentDef { name } => {
                pu_core::schedule_def::ScheduleTrigger::AgentDef { name: name.clone() }
            }
            ScheduleTriggerPayload::SwarmDef { name, vars } => {
                pu_core::schedule_def::ScheduleTrigger::SwarmDef {
                    name: name.clone(),
                    vars: vars.clone(),
                }
            }
            ScheduleTriggerPayload::InlinePrompt { prompt, agent } => {
                pu_core::schedule_def::ScheduleTrigger::InlinePrompt {
                    prompt: prompt.clone(),
                    agent: agent.clone(),
                }
            }
        }
    }

    // --- Scheduler ---

    /// Start a background task that periodically checks for due schedules and fires them.
    pub fn start_scheduler(self: &Arc<Self>) {
        let engine = Arc::clone(self);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(30));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                interval.tick().await;
                engine.scheduler_tick().await;
            }
        });
    }

    async fn scheduler_tick(&self) {
        let projects = self.registered_projects();
        for project_root in projects {
            let defs = {
                let pr = project_root.clone();
                match tokio::task::spawn_blocking(move || {
                    pu_core::schedule_def::list_schedule_defs(Path::new(&pr))
                })
                .await
                {
                    Ok(d) => d,
                    Err(_) => continue,
                }
            };

            let now = chrono::Utc::now();
            for def in defs {
                if !def.enabled {
                    continue;
                }
                if let Some(next_run) = def.next_run {
                    if next_run <= now {
                        self.fire_schedule(&def).await;
                        self.advance_schedule(def, now).await;
                    }
                }
            }
        }
    }

    async fn fire_schedule(&self, schedule: &pu_core::schedule_def::ScheduleDef) {
        let result = match &schedule.trigger {
            pu_core::schedule_def::ScheduleTrigger::AgentDef { name } => {
                // Resolve agent def and spawn
                let pr = schedule.project_root.clone();
                let agent_name = name.clone();
                let target = schedule.target.clone();
                let prompt = format!("Scheduled task: run agent def '{agent_name}' in {target}");
                self.handle_request(Request::Spawn {
                    project_root: pr,
                    prompt,
                    agent: "claude".to_string(),
                    name: None,
                    base: None,
                    root: true,
                    worktree: None,
                })
                .await
            }
            pu_core::schedule_def::ScheduleTrigger::SwarmDef { name, vars } => {
                self.handle_request(Request::RunSwarm {
                    project_root: schedule.project_root.clone(),
                    swarm_name: name.clone(),
                    vars: vars.clone(),
                })
                .await
            }
            pu_core::schedule_def::ScheduleTrigger::InlinePrompt { prompt, agent } => {
                self.handle_request(Request::Spawn {
                    project_root: schedule.project_root.clone(),
                    prompt: prompt.clone(),
                    agent: agent.clone(),
                    name: None,
                    base: None,
                    root: true,
                    worktree: None,
                })
                .await
            }
        };

        if let Response::Error { code, message } = result {
            tracing::warn!(
                schedule = schedule.name,
                code,
                message,
                "scheduled task failed"
            );
        } else {
            tracing::info!(schedule = schedule.name, "scheduled task fired");
        }
    }

    async fn advance_schedule(
        &self,
        mut schedule: pu_core::schedule_def::ScheduleDef,
        now: chrono::DateTime<chrono::Utc>,
    ) {
        let is_one_shot = schedule.recurrence == pu_core::schedule_def::Recurrence::None;
        if is_one_shot {
            schedule.enabled = false;
            schedule.next_run = None;
        } else {
            schedule.next_run = pu_core::schedule_def::next_occurrence(
                schedule.start_at,
                &schedule.recurrence,
                now,
            );
        }
        let pr = schedule.project_root.clone();
        let def = schedule;
        if let Err(e) = tokio::task::spawn_blocking(move || {
            let dir = paths::schedules_dir(Path::new(&pr));
            pu_core::schedule_def::save_schedule_def(&dir, &def)
        })
        .await
        .unwrap_or_else(|e| Err(std::io::Error::other(e)))
        {
            tracing::warn!(error = %e, "failed to advance schedule");
        }
    }

    // --- Scope resolution helper ---

    fn resolve_scope_dir(
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
    use pu_core::protocol::{Request, Response};
    use tempfile::TempDir;

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

    #[tokio::test(flavor = "current_thread")]
    async fn given_spawn_with_prompt_should_queue_initial_input() {
        let tmp = TempDir::new().unwrap();
        let project_root = tmp.path().join("project");
        std::fs::create_dir_all(&project_root).unwrap();
        let pr = project_root.to_string_lossy().to_string();

        let engine = Engine::new();
        engine
            .handle_request(Request::Init {
                project_root: pr.clone(),
            })
            .await;

        let resp = engine
            .handle_request(Request::Spawn {
                project_root: pr,
                prompt: "hello world".into(),
                agent: "terminal".into(),
                name: None,
                base: None,
                root: true,
                worktree: None,
            })
            .await;

        let agent_id = match resp {
            Response::SpawnResult { agent_id, .. } => agent_id,
            other => panic!("expected SpawnResult, got {other:?}"),
        };

        let initial_input = engine.take_initial_input(&agent_id).await;
        assert_eq!(initial_input, Some(b"hello world".to_vec()));
    }

    #[test]
    fn given_claude_prompt_should_inject_via_stdin() {
        assert!(Engine::should_inject_prompt_via_stdin(
            "claude", true, "hello"
        ));
    }

    #[test]
    fn given_codex_prompt_should_not_inject_via_stdin() {
        assert!(!Engine::should_inject_prompt_via_stdin(
            "codex", true, "hello"
        ));
    }

    #[test]
    fn given_non_interactive_agent_should_not_inject_via_stdin() {
        assert!(!Engine::should_inject_prompt_via_stdin(
            "terminal", false, "hello"
        ));
    }

    #[test]
    fn given_opencode_without_configured_flag_should_use_prompt_flag() {
        assert_eq!(
            Engine::resolved_prompt_flag("opencode", None),
            Some("--prompt".to_string())
        );
    }

    #[test]
    fn given_codex_without_configured_flag_should_use_positional_prompt() {
        assert_eq!(Engine::resolved_prompt_flag("codex", None), None);
    }

    #[test]
    fn given_configured_prompt_flag_should_be_preserved() {
        assert_eq!(
            Engine::resolved_prompt_flag("codex", Some("--prompt")),
            Some("--prompt".to_string())
        );
    }
}
