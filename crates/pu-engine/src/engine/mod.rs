mod agent_lifecycle;
mod definitions;
mod helpers;
mod pty_operations;
mod scheduler;
mod session_repair;
mod spawn;
mod status;
mod subscriptions;
mod trigger_executor;
mod worktree_ops;

#[cfg(test)]
use session_repair::repair_session_file;

use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::Mutex;

use pu_core::config;
use pu_core::paths;
use pu_core::protocol::{AgentConfigInfo, GridCommand, PROTOCOL_VERSION, Request, Response};
use pu_core::types::Manifest;
use tokio::sync::OnceCell;

use crate::pty_manager::{
    AgentHandle, NativePtyHost, descendants_from_tree, snapshot_process_tree,
};

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

    /// Drain every active session and kill its process group (SIGTERM, brief wait, SIGKILL).
    /// Used by the managed-mode parent-died path so a force-quit of the macOS app reaps
    /// agents and their grandchildren (vitest/node workers, dev servers) before the
    /// daemon exits — `process::exit(0)` skips `Drop`, so this must run explicitly.
    pub async fn kill_all_sessions(&self, grace: Duration) {
        let handles: Vec<AgentHandle> = {
            let mut sessions = self.sessions.lock().await;
            sessions.drain().map(|(_, h)| h).collect()
        };
        if handles.is_empty() {
            return;
        }

        // One `ps` snapshot for all handles — cheaper than one per agent.
        let tree = snapshot_process_tree().await;
        let all_descendants: Vec<i32> = handles
            .iter()
            .filter_map(|h| i32::try_from(h.pid).ok())
            .flat_map(|pid| descendants_from_tree(&tree, pid))
            .collect();

        for handle in &handles {
            if let Ok(pid) = i32::try_from(handle.pid) {
                unsafe {
                    libc::killpg(pid, libc::SIGTERM);
                }
            }
        }
        for &desc in &all_descendants {
            unsafe {
                libc::kill(desc, libc::SIGTERM);
            }
        }

        let deadline = tokio::time::Instant::now() + grace;
        loop {
            let all_done = handles.iter().all(|h| h.exit_rx.borrow().is_some());
            if all_done || tokio::time::Instant::now() >= deadline {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }

        for handle in &handles {
            if handle.exit_rx.borrow().is_some() {
                continue;
            }
            if let Ok(pid) = i32::try_from(handle.pid) {
                unsafe {
                    libc::killpg(pid, libc::SIGKILL);
                }
            }
        }
        for &desc in &all_descendants {
            unsafe {
                libc::kill(desc, libc::SIGKILL);
            }
        }
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
}

impl Drop for Engine {
    fn drop(&mut self) {
        // Kill the whole process group of each child (PID == PGID via setsid at spawn)
        // so descendants like vitest/node workers don't orphan to launchd.
        if let Ok(sessions) = self.sessions.try_lock() {
            for handle in sessions.values() {
                unsafe {
                    libc::killpg(handle.pid as i32, libc::SIGKILL);
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
