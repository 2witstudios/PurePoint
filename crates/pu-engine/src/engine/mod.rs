mod definitions;
mod pty_operations;
mod scheduler;
mod session_repair;
mod subscriptions;
mod trigger_executor;

use session_repair::inject_initial_prompt;
#[cfg(test)]
use session_repair::repair_session_file;

use std::collections::{HashMap, HashSet};
use std::io::Write;
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
    AgentConfigInfo, AgentStatusReport, GridCommand, KillTarget, PROTOCOL_VERSION, Request,
    Response, SuspendTarget,
};
use pu_core::types::{AgentEntry, AgentStatus, Manifest, WorktreeEntry, WorktreeStatus};
use tokio::sync::OnceCell;

use crate::agent_monitor;
use crate::daemon_lifecycle;
use crate::git;
use crate::pty_manager::{AgentHandle, NativePtyHost, SpawnConfig};

/// Parameters for spawning an agent, extracted to avoid too many positional args.
pub(super) struct SpawnParams {
    project_root: String,
    prompt: String,
    agent_type: String,
    name: Option<String>,
    base: Option<String>,
    root: bool,
    worktree: Option<String>,
    terminal_command: Option<String>,
    /// Skip auto-mode launch args for this spawn. One-off override;
    /// does not affect resume (resume always reads from config).
    no_auto: bool,
    /// Extra CLI args from --agent-args, appended after launch args.
    extra_args: Vec<String>,
    plan_mode: bool,
    no_trigger: bool,
    /// Name of trigger to bind (from --trigger flag)
    trigger: Option<String>,
}

pub(super) struct SaveTriggerParams {
    project_root: String,
    name: String,
    description: Option<String>,
    on: String,
    sequence: Vec<pu_core::protocol::TriggerActionPayload>,
    variables: std::collections::HashMap<String, String>,
    scope: String,
}

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

/// Build a `ConfigReport` response from a loaded config.
/// Filters out the terminal agent (no launch args to configure).
fn config_to_report(cfg: &pu_core::types::Config) -> Response {
    let agents = cfg
        .agents
        .iter()
        .filter(|(name, _)| name.as_str() != "terminal")
        .map(|(name, ac)| {
            let resolved = pu_core::types::resolved_launch_args(name, ac.launch_args.as_deref());
            AgentConfigInfo {
                name: ac.name.clone(),
                command: ac.command.clone(),
                launch_args: ac.launch_args.clone(),
                resolved_launch_args: resolved,
                interactive: ac.interactive,
            }
        })
        .collect();
    Response::ConfigReport {
        default_agent: cfg.default_agent.clone(),
        agents,
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
                    let mut sessions = sessions.lock().await;
                    let dead: Vec<String> = sessions
                        .iter()
                        .filter(|(_, handle)| handle.exit_rx.borrow().is_some())
                        .map(|(id, _)| id.clone())
                        .collect();
                    for id in &dead {
                        sessions.remove(id);
                    }
                    dead
                };
                if !dead_ids.is_empty() {
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
            | Request::CreateWorktree { project_root, .. }
            | Request::Status { project_root, .. }
            | Request::Kill { project_root, .. }
            | Request::ListTemplates { project_root }
            | Request::ListAgentDefs { project_root }
            | Request::ListSwarmDefs { project_root }
            | Request::ListSchedules { project_root }
            | Request::SaveSchedule { project_root, .. }
            | Request::EnableSchedule { project_root, .. }
            | Request::DisableSchedule { project_root, .. }
            | Request::ListTriggers { project_root }
            | Request::SaveTrigger { project_root, .. }
            | Request::EvaluateGate { project_root, .. }
            | Request::Diff { project_root, .. }
            | Request::GetConfig { project_root }
            | Request::UpdateAgentConfig { project_root, .. }
            | Request::Pulse { project_root, .. }
            | Request::AssignTrigger { project_root, .. } => {
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
            Request::AssignTrigger {
                project_root,
                agent_id,
                trigger_name,
            } => {
                self.handle_assign_trigger(&project_root, &agent_id, &trigger_name)
                    .await
            }
            Request::GetConfig { project_root } => self.handle_get_config(&project_root).await,
            Request::UpdateAgentConfig {
                project_root,
                agent_name,
                launch_args,
            } => {
                self.handle_update_agent_config(&project_root, &agent_name, launch_args)
                    .await
            }
            Request::Shutdown => Response::ShuttingDown,
            Request::Status {
                project_root,
                agent_id,
            } => self.handle_status(&project_root, agent_id.as_deref()).await,
            Request::SpawnShell { cwd } => self.handle_spawn_shell(&cwd).await,
            Request::Spawn {
                project_root,
                prompt,
                agent,
                name,
                base,
                root,
                worktree,
                command,
                no_auto,
                extra_args,
                plan_mode,
                no_trigger,
                trigger,
            } => {
                self.handle_spawn(SpawnParams {
                    project_root,
                    prompt,
                    agent_type: agent,
                    name,
                    base,
                    root,
                    worktree,
                    terminal_command: command,
                    no_auto,
                    extra_args,
                    plan_mode,
                    no_trigger,
                    trigger,
                })
                .await
            }
            Request::CreateWorktree {
                project_root,
                name,
                base,
            } => self.handle_create_worktree(&project_root, name, base).await,
            Request::Kill {
                project_root,
                target,
                exclude,
            } => self.handle_kill(&project_root, target, &exclude).await,
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
            Request::Input {
                agent_id,
                data,
                submit,
            } => self.handle_input(&agent_id, &data, submit).await,
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
                command,
            } => {
                self.handle_save_template(
                    &project_root,
                    &name,
                    &description,
                    &agent,
                    &body,
                    &scope,
                    command,
                )
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
                command,
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
                    command,
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
                root,
                agent_name,
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
                    root,
                    agent_name,
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
            // Trigger CRUD
            Request::ListTriggers { project_root } => {
                self.handle_list_triggers(&project_root).await
            }
            Request::GetTrigger { project_root, name } => {
                self.handle_get_trigger(&project_root, &name).await
            }
            Request::SaveTrigger {
                project_root,
                name,
                description,
                on,
                sequence,
                variables,
                scope,
            } => {
                self.handle_save_trigger(SaveTriggerParams {
                    project_root,
                    name,
                    description,
                    on,
                    sequence,
                    variables,
                    scope,
                })
                .await
            }
            Request::DeleteTrigger {
                project_root,
                name,
                scope,
            } => {
                self.handle_delete_trigger(&project_root, &name, &scope)
                    .await
            }
            Request::EvaluateGate {
                event,
                project_root,
                worktree_path,
            } => {
                self.handle_evaluate_gate(&event, &project_root, &worktree_path)
                    .await
            }
            Request::Diff {
                project_root,
                worktree_id,
                stat,
            } => {
                self.handle_diff(&project_root, worktree_id.as_deref(), stat)
                    .await
            }
            Request::Pulse { project_root } => self.handle_pulse(&project_root).await,
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

    async fn handle_get_config(&self, project_root: &str) -> Response {
        let pr = project_root.to_string();
        tokio::task::spawn_blocking(move || {
            let root = Path::new(&pr);
            match config::load_config_strict(root) {
                Ok(cfg) => config_to_report(&cfg),
                Err(e) => Response::Error {
                    code: e.code().into(),
                    message: e.to_string(),
                },
            }
        })
        .await
        .unwrap_or_else(|e| Response::Error {
            code: "INTERNAL_ERROR".into(),
            message: format!("task join error: {e}"),
        })
    }

    async fn handle_update_agent_config(
        &self,
        project_root: &str,
        agent_name: &str,
        launch_args: Option<Vec<String>>,
    ) -> Response {
        let pr = project_root.to_string();
        let name = agent_name.to_string();
        tokio::task::spawn_blocking(move || {
            let root = Path::new(&pr);
            match config::update_agent_config(root, &name, launch_args) {
                Ok(cfg) => config_to_report(&cfg),
                Err(e) => Response::Error {
                    code: e.code().into(),
                    message: e.to_string(),
                },
            }
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
    fn build_agent_status_report(
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
                let idle_seconds = Some(handle.output_buffer.content_idle_seconds());
                (status, exit_code, idle_seconds)
            }
            // No live session — use manifest (agent already exited/killed/etc.)
            None => (agent.status, agent.exit_code, None),
        }
    }

    /// Spawn a bare shell (no project, no manifest, no config).
    /// Used by Point Guard for a root terminal at the given cwd.
    async fn handle_spawn_shell(&self, cwd: &str) -> Response {
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_string());
        let agent_id = pu_core::id::agent_id();

        let env = self.agent_env().await;
        let spawn_config = SpawnConfig {
            command: shell,
            args: vec!["-l".to_string()],
            cwd: cwd.to_string(),
            env,
            env_remove: vec![],
            cols: 120,
            rows: 40,
        };

        let handle = match self.pty_host.spawn(spawn_config).await {
            Ok(h) => h,
            Err(e) => {
                return Response::Error {
                    code: "SPAWN_FAILED".into(),
                    message: format!("failed to spawn shell: {e}"),
                };
            }
        };

        // Start exit monitor (cleans up session map when shell exits)
        let exit_rx = handle.exit_rx.clone();
        let sessions = self.sessions.clone();
        let aid = agent_id.clone();
        tokio::spawn(async move {
            let mut rx = exit_rx;
            while rx.changed().await.is_ok() {
                if rx.borrow().is_some() {
                    break;
                }
            }
            sessions.lock().await.remove(&aid);
        });

        self.sessions.lock().await.insert(agent_id.clone(), handle);

        Response::SpawnResult {
            worktree_id: None,
            agent_id,
            status: AgentStatus::Streaming,
        }
    }

    async fn handle_spawn(&self, params: SpawnParams) -> Response {
        let SpawnParams {
            project_root,
            prompt,
            agent_type,
            name,
            base,
            root,
            worktree,
            terminal_command,
            no_auto,
            extra_args,
            plan_mode,
            no_trigger,
            trigger: trigger_param,
        } = params;
        let root_path = Path::new(&project_root);

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
        let agent_cfg = match config::resolve_agent(&cfg, &agent_type) {
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
            let Some(raw) = name else {
                return Response::Error {
                    code: "INVALID_ARGUMENT".into(),
                    message: "worktree spawn requires a name".into(),
                };
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
        let base_branch = match base {
            Some(b) => b,
            None => git::resolve_base_ref(root_path, "HEAD")
                .await
                .unwrap_or_else(|_| "HEAD".into()),
        };

        // Normalize empty command to None
        let terminal_command = terminal_command.filter(|c| !c.is_empty());

        // Plan mode requires a prompt-driven agent that understands EnterPlanMode.
        // Reject early for terminal agents or terminal_command spawns where the
        // prefix would be meaningless or actively harmful.
        if plan_mode
            && (prompt.is_empty() || terminal_command.is_some() || agent_type == "terminal")
        {
            return Response::Error {
                code: "INVALID_ARGUMENT".into(),
                message: "plan mode requires a prompt-driven non-terminal agent".into(),
            };
        }

        // When plan_mode is active, prefix the prompt with instructions to enter plan mode.
        // This keeps bypass permissions as the base while guiding the agent into plan mode
        // via its own tool (EnterPlanMode) rather than conflicting CLI flags.
        let prompt = if plan_mode {
            format!(
                "[PLAN MODE] You MUST call the EnterPlanMode tool immediately before doing anything else. \
                 Do not read files, do not explore — call EnterPlanMode first. \
                 Once in plan mode, research and plan before making changes.\n\n{prompt}"
            )
        } else {
            prompt.to_string()
        };
        let prompt = &prompt;

        // When a terminal command is set, it becomes the PTY process directly
        let (command, args, session_id, inject_prompt_via_stdin) = if let Some(ref cmd) =
            terminal_command
        {
            let has_metacharacters = cmd.contains('|')
                || cmd.contains("&&")
                || cmd.contains(';')
                || cmd.contains('>')
                || cmd.contains('<')
                || cmd.contains('$');

            let (cmd_bin, cmd_args) = if has_metacharacters {
                let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
                (shell, vec!["-c".to_string(), cmd.clone()])
            } else {
                let parts: Vec<&str> = cmd.split_whitespace().collect();
                if parts.is_empty() {
                    // Shouldn't happen after filter, but handle gracefully
                    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
                    (shell, vec![])
                } else {
                    (
                        parts[0].to_string(),
                        parts[1..].iter().map(ToString::to_string).collect(),
                    )
                }
            };
            (cmd_bin, cmd_args, None, false)
        } else {
            // Standard agent flow
            let (command, cmd_args) = match Self::parse_agent_command(&agent_cfg, &agent_type) {
                Ok(v) => v,
                Err(e) => return e,
            };
            let mut args = cmd_args;

            // Add agent-type-specific launch args from config (or defaults).
            // --no-auto skips only the built-in defaults; explicit user-configured
            // launchArgs are always applied.
            let launch_args = if no_auto && agent_cfg.launch_args.is_none() {
                Vec::new()
            } else {
                pu_core::types::resolved_launch_args(&agent_type, agent_cfg.launch_args.as_deref())
            };
            if launch_args.is_empty() && agent_cfg.launch_args.is_some() {
                tracing::info!(agent_type, "auto-mode disabled via config (launchArgs: [])");
            }
            for arg in launch_args.into_iter().rev() {
                if !args.iter().any(|a| a == &arg) {
                    args.insert(0, arg);
                }
            }

            // Append extra args from --agent-args (always applied, even with --no-auto)
            args.extend(extra_args.iter().cloned());

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
                Self::should_inject_prompt_via_stdin(&agent_type, agent_cfg.interactive, prompt);
            if !inject_prompt_via_stdin && !prompt.is_empty() {
                let prompt_flag =
                    Self::resolved_prompt_flag(&agent_type, agent_cfg.prompt_flag.as_deref());
                if let Some(flag) = prompt_flag {
                    args.push(flag);
                    args.push(prompt.to_string());
                } else {
                    // Default prompt style is positional (for example codex [PROMPT]).
                    args.push(prompt.to_string());
                }
            }

            (command, args, session_id, inject_prompt_via_stdin)
        };

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

            // Install git hooks for trigger gate enforcement
            if let Err(e) = git::install_hooks(&wt_path, root_path).await {
                tracing::warn!("failed to install git hooks in worktree: {e}");
            }

            // Copy env files (e.g., .env, .env.local) into new worktree
            for env_file in &cfg.env_files {
                let src = root_path.join(env_file);
                let dst = wt_path.join(env_file);
                match tokio::fs::copy(&src, &dst).await {
                    Ok(_) => {}
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound && !src.exists() => {} // source doesn't exist, skip
                    Err(e) => tracing::warn!("failed to copy {env_file} to worktree: {e}"),
                }
            }

            (wt_path.to_string_lossy().to_string(), Some(wt_id))
        };

        // Spawn PTY process
        let mut env = self.agent_env().await;
        env.push(("PU_AGENT_ID".into(), agent_id.clone()));
        let spawn_config = SpawnConfig {
            command,
            args,
            cwd: cwd.clone(),
            env,
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
            let prompt_bytes = prompt.as_bytes().to_vec();
            let pending = self.pending_initial_inputs.clone();
            pending
                .lock()
                .await
                .insert(agent_id.clone(), prompt_bytes.clone());

            let output_buffer = handle.output_buffer.clone();
            let master_fd = handle.master_fd();
            let mut exit_rx = handle.exit_rx.clone();
            let pty_host = NativePtyHost::new();
            let aid = agent_id.clone();

            tokio::spawn(async move {
                let mut watcher = output_buffer.subscribe();
                let timeout = tokio::time::sleep(Duration::from_millis(1800));
                tokio::pin!(timeout);

                loop {
                    tokio::select! {
                        _ = &mut timeout => {
                            // Fallback or quiet-period expired — inject now
                            break;
                        }
                        Ok(()) = exit_rx.changed() => {
                            // Process exited before we could inject — abort
                            pending.lock().await.remove(&aid);
                            tracing::debug!(agent_id = %aid, "prompt injection aborted: process exited");
                            return;
                        }
                        Ok(()) = watcher.changed() => {
                            // Got output — reset to a 450ms quiet period
                            timeout
                                .as_mut()
                                .reset(tokio::time::Instant::now() + Duration::from_millis(450));
                        }
                    }
                }

                // Inject the prompt
                if inject_initial_prompt(&pty_host, &master_fd, &aid, &prompt_bytes).await {
                    tracing::debug!(agent_id = %aid, "prompt injected at spawn time");
                } else {
                    tracing::warn!(agent_id = %aid, "failed to inject prompt at spawn time");
                }
                pending.lock().await.remove(&aid);
            });
        }

        let pid = handle.pid;

        // Store handle in session map BEFORE writing manifest.
        // ManifestWatcher in Swift fires on manifest write and immediately
        // tries to attach — the session must already be in the map.
        self.sessions.lock().await.insert(agent_id.clone(), handle);

        // Bind trigger if explicitly specified via --trigger <name>
        let (trigger_name, trigger_total) = if no_trigger {
            (None, None)
        } else if let Some(ref name) = trigger_param {
            let pr = project_root.to_string();
            let name_clone = name.clone();
            let found = tokio::task::spawn_blocking(move || {
                let triggers = pu_core::trigger_def::triggers_for_event(
                    Path::new(&pr),
                    &pu_core::trigger_def::TriggerEvent::AgentIdle,
                );
                triggers
                    .into_iter()
                    .find(|t| t.name == name_clone)
                    .map(|t| {
                        let len = t.sequence.len() as u32;
                        (t.name, len)
                    })
            })
            .await
            .unwrap_or(None);
            match found {
                Some((tname, total)) if total > 0 => (Some(tname), Some(total)),
                Some(_) => {
                    return Response::Error {
                        code: "INVALID_TRIGGER".into(),
                        message: format!("trigger '{name}' has empty sequence"),
                    };
                }
                None => {
                    return Response::Error {
                        code: "NOT_FOUND".into(),
                        message: format!("trigger '{name}' not found"),
                    };
                }
            }
        } else {
            (None, None)
        };

        // Update manifest
        let agent_entry = AgentEntry {
            id: agent_id.clone(),
            name: agent_name.clone(),
            agent_type,
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
            command: terminal_command,
            plan_mode,
            trigger_seq_index: trigger_name.as_ref().map(|_| 0),
            trigger_state: trigger_name
                .as_ref()
                .map(|_| pu_core::types::TriggerState::Active),
            trigger_total,
            gate_attempts: trigger_name.as_ref().map(|_| 0),
            no_trigger,
            trigger_name: trigger_name.clone(),
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

        self.notify_status_change(&project_root).await;

        Response::SpawnResult {
            worktree_id,
            agent_id,
            status: AgentStatus::Streaming,
        }
    }

    async fn handle_create_worktree(
        &self,
        project_root: &str,
        name: Option<String>,
        base: Option<String>,
    ) -> Response {
        let root_path = Path::new(project_root);

        // Ensure initialized
        if !paths::manifest_path(root_path).exists() {
            return Response::Error {
                code: "NOT_INITIALIZED".into(),
                message: "not initialized — run `pu init` first".into(),
            };
        }

        // Load config for env_files
        let cfg = match config::load_config_strict(root_path) {
            Ok(c) => c,
            Err(e) => {
                return Response::Error {
                    code: "CONFIG_ERROR".into(),
                    message: format!("failed to load config: {e}"),
                };
            }
        };

        // Resolve name
        let Some(raw) = name else {
            return Response::Error {
                code: "INVALID_ARGUMENT".into(),
                message: "worktree creation requires a name".into(),
            };
        };
        let worktree_name = pu_core::id::normalize_worktree_name(&raw);
        if worktree_name.is_empty() {
            return Response::Error {
                code: "INVALID_ARGUMENT".into(),
                message: "worktree creation requires a name".into(),
            };
        }

        let base_branch = match base {
            Some(b) => b,
            None => git::resolve_base_ref(root_path, "HEAD")
                .await
                .unwrap_or_else(|_| "HEAD".into()),
        };
        let wt_id = pu_core::id::worktree_id();
        let wt_path = paths::worktree_path(root_path, &wt_id);
        let branch = format!("pu/{worktree_name}");
        let rollback_branch = branch.clone();

        if let Err(e) = git::create_worktree(root_path, &wt_path, &branch, &base_branch).await {
            return Response::Error {
                code: "CREATE_WORKTREE_FAILED".into(),
                message: format!("failed to create worktree: {e}"),
            };
        }

        // Install git hooks for trigger gate enforcement
        if let Err(e) = git::install_hooks(&wt_path, root_path).await {
            tracing::warn!("failed to install git hooks in worktree: {e}");
        }

        // Copy env files into new worktree
        for env_file in &cfg.env_files {
            let src = root_path.join(env_file);
            let dst = wt_path.join(env_file);
            match tokio::fs::copy(&src, &dst).await {
                Ok(_) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound && !src.exists() => {}
                Err(e) => tracing::warn!("failed to copy {env_file} to worktree: {e}"),
            }
        }

        // Write manifest entry (worktree only, no agents)
        let cwd = wt_path.to_string_lossy().to_string();
        let wt_id_clone = wt_id.clone();
        let manifest_result = manifest::update_manifest(root_path, move |mut m| {
            m.worktrees
                .entry(wt_id_clone.clone())
                .or_insert_with(|| WorktreeEntry {
                    id: wt_id_clone,
                    name: worktree_name.clone(),
                    path: cwd,
                    branch,
                    base_branch: Some(base_branch.clone()),
                    status: WorktreeStatus::Active,
                    agents: IndexMap::new(),
                    created_at: chrono::Utc::now(),
                    merged_at: None,
                });
            m
        });

        if let Err(e) = manifest_result {
            // Rollback: remove worktree + branch
            self.rollback_worktree(root_path, Some(&wt_id), Some(&rollback_branch))
                .await;
            return Response::Error {
                code: "CREATE_WORKTREE_FAILED".into(),
                message: format!("failed to update manifest: {e}"),
            };
        }

        self.notify_status_change(project_root).await;

        Response::CreateWorktreeResult { worktree_id: wt_id }
    }

    async fn handle_kill(
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
        self.kill_agents(&agent_ids).await;

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
                code: "INTERNAL_ERROR".into(),
                message: format!("rename task failed: {e}"),
            },
        }
    }

    async fn handle_assign_trigger(
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
    fn build_resume_command(
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

    /// Remove pending inputs and session handles for the given agent IDs, then kill their
    /// PTY processes. Returns the extracted handles (for callers that need exit codes).
    async fn kill_agents(&self, agent_ids: &[String]) -> Vec<(String, AgentHandle)> {
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

    /// On daemon restart, reconcile agents that appear alive in the manifest but have no
    /// live process. Resumable agents (claude, codex, opencode) with a session_id get marked
    /// suspended so the Swift side can auto-resume them. Others get marked Broken.
    /// Called synchronously inside handle_init so state is correct before the first status read.
    fn reconcile_agents_on_init(project_root: &str) {
        let root = Path::new(project_root);
        let Ok(m) = manifest::read_manifest(root) else {
            return;
        };
        let is_stale = |a: &AgentEntry| {
            !a.suspended && matches!(a.status, AgentStatus::Streaming | AgentStatus::Waiting)
        };
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
        let Ok(m) = manifest::read_manifest(root) else {
            return;
        };
        let needs_reap = |a: &AgentEntry| {
            !a.suspended
                && matches!(a.status, AgentStatus::Streaming | AgentStatus::Waiting)
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
                    && matches!(agent.status, AgentStatus::Streaming | AgentStatus::Waiting)
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

    async fn handle_pulse(&self, project_root: &str) -> Response {
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

    async fn handle_diff(
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

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn given_spawn_with_prompt_should_inject_in_background() {
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
                command: None,
                no_auto: false,
                extra_args: vec![],
                plan_mode: false,
                no_trigger: false,
                trigger: None,
            })
            .await;

        let agent_id = match resp {
            Response::SpawnResult { agent_id, .. } => agent_id,
            other => panic!("expected SpawnResult, got {other:?}"),
        };

        // The background task should eventually drain pending_initial_inputs.
        // Allow up to 5s for the injection to complete (includes readiness
        // timeout + chunked write delays).
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        loop {
            let still_pending = engine
                .pending_initial_inputs
                .lock()
                .await
                .contains_key(&agent_id);
            if !still_pending {
                break;
            }
            if tokio::time::Instant::now() >= deadline {
                panic!("prompt was never consumed from pending_initial_inputs");
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
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

    #[test]
    fn given_claude_build_resume_with_default_launch_args_should_include_yolo() {
        // given
        let engine = Engine::new();
        let agent_cfg = pu_core::types::AgentConfig {
            name: "claude".into(),
            command: "claude".into(),
            prompt_flag: None,
            interactive: true,
            launch_args: None, // use defaults
        };

        // when
        let (cmd, args, sid) = engine
            .build_resume_command("claude", &agent_cfg, Some("sess-123"))
            .unwrap();

        // then
        assert_eq!(cmd, "claude");
        assert!(args.contains(&"--dangerously-skip-permissions".to_string()));
        assert!(args.contains(&"--resume".to_string()));
        assert!(args.contains(&"sess-123".to_string()));
        assert_eq!(sid, Some("sess-123".to_string()));
    }

    #[test]
    fn given_claude_build_resume_with_empty_launch_args_should_omit_yolo() {
        // given: user configured launchArgs: [] to disable auto-mode
        let engine = Engine::new();
        let agent_cfg = pu_core::types::AgentConfig {
            name: "claude".into(),
            command: "claude".into(),
            prompt_flag: None,
            interactive: true,
            launch_args: Some(vec![]),
        };

        // when
        let (cmd, args, _) = engine
            .build_resume_command("claude", &agent_cfg, Some("sess-456"))
            .unwrap();

        // then
        assert_eq!(cmd, "claude");
        assert!(!args.contains(&"--dangerously-skip-permissions".to_string()));
        assert!(args.contains(&"--resume".to_string()));
    }

    #[test]
    fn given_codex_build_resume_with_custom_launch_args_should_place_before_subcommand() {
        // given
        let engine = Engine::new();
        let agent_cfg = pu_core::types::AgentConfig {
            name: "codex".into(),
            command: "codex".into(),
            prompt_flag: None,
            interactive: true,
            launch_args: Some(vec!["--approval-mode=full-auto".into()]),
        };

        // when
        let (cmd, args, _) = engine
            .build_resume_command("codex", &agent_cfg, None)
            .unwrap();

        // then — top-level flags must precede the subcommand
        assert_eq!(cmd, "codex");
        assert_eq!(args, vec!["--approval-mode=full-auto", "resume", "--last"]);
    }

    #[test]
    fn given_codex_build_resume_with_defaults_should_place_full_auto_before_subcommand() {
        // given
        let engine = Engine::new();
        let agent_cfg = pu_core::types::AgentConfig {
            name: "codex".into(),
            command: "codex".into(),
            prompt_flag: None,
            interactive: true,
            launch_args: None, // use defaults
        };

        // when
        let (cmd, args, _) = engine
            .build_resume_command("codex", &agent_cfg, None)
            .unwrap();

        // then — --full-auto is a top-level flag, must come before `resume`
        assert_eq!(cmd, "codex");
        assert_eq!(args, vec!["--full-auto", "resume", "--last"]);
    }

    /// Test the no_auto + launch_args interaction logic used in handle_spawn.
    /// When no_auto is true and launch_args is None (defaults), launch args should be empty.
    /// When no_auto is true but launch_args is explicitly configured, they should be preserved.
    #[test]
    fn given_no_auto_with_default_launch_args_should_produce_empty() {
        let agent_cfg = pu_core::types::AgentConfig {
            name: "claude".into(),
            command: "claude".into(),
            prompt_flag: None,
            interactive: true,
            launch_args: None,
        };
        let no_auto = true;
        let launch_args = if no_auto && agent_cfg.launch_args.is_none() {
            Vec::new()
        } else {
            pu_core::types::resolved_launch_args("claude", agent_cfg.launch_args.as_deref())
        };
        assert!(
            launch_args.is_empty(),
            "--no-auto should skip default launch args"
        );
    }

    #[test]
    fn given_no_auto_with_explicit_launch_args_should_preserve_them() {
        let agent_cfg = pu_core::types::AgentConfig {
            name: "claude".into(),
            command: "claude".into(),
            prompt_flag: None,
            interactive: true,
            launch_args: Some(vec!["--verbose".into()]),
        };
        let no_auto = true;
        let launch_args = if no_auto && agent_cfg.launch_args.is_none() {
            Vec::new()
        } else {
            pu_core::types::resolved_launch_args("claude", agent_cfg.launch_args.as_deref())
        };
        assert_eq!(
            launch_args,
            vec!["--verbose"],
            "--no-auto should not affect explicit launch args"
        );
    }

    #[test]
    fn given_snapshot_collision_should_nullify_message_id() {
        // given: a session file where a file-history-snapshot reuses a real message uuid
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("session.jsonl");
        let content = [
            r#"{"uuid":"u1","type":"summary","parentUuid":null}"#,
            r#"{"uuid":"u2","type":"assistant","message":{"content":"hello"}}"#,
            r#"{"type":"file-history-snapshot","messageId":"u2","data":{}}"#,
        ]
        .join("\n")
            + "\n";
        std::fs::write(&path, &content).unwrap();

        // when
        let repaired = repair_session_file(&path);

        // then
        assert!(repaired);
        let result = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<serde_json::Value> = result
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        // The snapshot's messageId should be null
        assert_eq!(lines[2]["messageId"], serde_json::Value::Null);
        // Backup should exist
        assert!(tmp.path().join("session.jsonl.bak").exists());
    }

    #[test]
    fn given_broken_parent_uuid_should_fix_reference() {
        // given: a session file where an entry's parentUuid points to a non-existent uuid
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("session.jsonl");
        let content = [
            r#"{"uuid":"u1","type":"summary","parentUuid":null}"#,
            r#"{"uuid":"u2","parentUuid":"u1","type":"assistant"}"#,
            r#"{"uuid":"u3","parentUuid":"DOES_NOT_EXIST","type":"assistant"}"#,
        ]
        .join("\n")
            + "\n";
        std::fs::write(&path, &content).unwrap();

        // when
        let repaired = repair_session_file(&path);

        // then
        assert!(repaired);
        let result = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<serde_json::Value> = result
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        // u3's parentUuid should now point to u2 (nearest preceding)
        assert_eq!(lines[2]["parentUuid"], "u2");
    }

    #[test]
    fn given_disconnected_root_should_stitch() {
        // given: a session file with two entries having parentUuid: null (disconnected roots)
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("session.jsonl");
        let content = [
            r#"{"uuid":"u1","type":"summary","parentUuid":null}"#,
            r#"{"uuid":"u2","parentUuid":"u1","type":"assistant"}"#,
            r#"{"uuid":"u3","parentUuid":null,"type":"assistant"}"#,
        ]
        .join("\n")
            + "\n";
        std::fs::write(&path, &content).unwrap();

        // when
        let repaired = repair_session_file(&path);

        // then
        assert!(repaired);
        let result = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<serde_json::Value> = result
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        // u3's parentUuid should now point to u2 (nearest preceding)
        assert_eq!(lines[2]["parentUuid"], "u2");
    }

    // --- build_resume_command tests ---

    fn dummy_agent_cfg(name: &str) -> pu_core::types::AgentConfig {
        pu_core::types::AgentConfig {
            name: name.into(),
            command: name.into(),
            prompt_flag: None,
            interactive: true,
            launch_args: None,
        }
    }

    #[test]
    fn given_claude_resume_should_use_bypass_and_resume() {
        let engine = Engine::new();
        let cfg = dummy_agent_cfg("claude");
        let (cmd, args, sid) = engine
            .build_resume_command("claude", &cfg, Some("sess-1"))
            .unwrap();
        assert_eq!(cmd, "claude");
        assert!(args.contains(&"--dangerously-skip-permissions".to_string()));
        assert!(args.contains(&"--resume".to_string()));
        assert!(args.contains(&"sess-1".to_string()));
        assert_eq!(sid, Some("sess-1".to_string()));
    }

    #[test]
    fn given_codex_resume_should_use_full_auto() {
        let engine = Engine::new();
        let cfg = dummy_agent_cfg("codex");
        let (cmd, args, _) = engine.build_resume_command("codex", &cfg, None).unwrap();
        assert_eq!(cmd, "codex");
        assert!(args.contains(&"--full-auto".to_string()));
        assert!(args.contains(&"resume".to_string()));
        assert!(args.contains(&"--last".to_string()));
    }

    #[test]
    fn given_opencode_resume_should_use_continue() {
        let engine = Engine::new();
        let cfg = dummy_agent_cfg("opencode");
        let (cmd, args, _) = engine.build_resume_command("opencode", &cfg, None).unwrap();
        assert_eq!(cmd, "opencode");
        assert!(args.contains(&"--continue".to_string()));
        assert!(!args.contains(&"--agent".to_string()));
    }

    #[test]
    fn given_intact_file_should_not_modify() {
        // given: a perfectly valid session file
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("session.jsonl");
        let content = [
            r#"{"uuid":"u1","type":"summary","parentUuid":null}"#,
            r#"{"uuid":"u2","parentUuid":"u1","type":"assistant"}"#,
            r#"{"uuid":"u3","parentUuid":"u2","type":"assistant"}"#,
        ]
        .join("\n")
            + "\n";
        std::fs::write(&path, &content).unwrap();

        // when
        let repaired = repair_session_file(&path);

        // then: no changes needed
        assert!(!repaired);
        // No backup file should be created
        assert!(!tmp.path().join("session.jsonl.bak").exists());
    }
}
