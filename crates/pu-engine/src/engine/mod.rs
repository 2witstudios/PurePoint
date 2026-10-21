mod agent_lifecycle;
mod attach_file;
mod build_cache;
mod channel;
mod definitions;
mod fs_ops;
mod helpers;
mod inventory;
mod pty_operations;
pub(crate) use pty_operations::{AttachmentCursor, AttachmentUpdate, PtySize};
mod read;
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
use pu_core::types::{Manifest, format_launch_args};
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
    /// Launch the bare command, ignoring configured launch args. One-off override;
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
    sizes: Mutex<HashMap<String, pty_operations::SizeState>>,
    signal_states: crate::signals::SignalStates,
    native_sources: Arc<crate::signals::NativeSources>,
    project_registry_path: Option<std::path::PathBuf>,
    project_registry_updates: Arc<Mutex<()>>,
    start_time: Instant,
    pty_host: NativePtyHost,
    sessions: Arc<Mutex<HashMap<String, AgentHandle>>>,
    pending_initial_inputs: Arc<Mutex<HashMap<String, Vec<u8>>>>,
    /// Per-agent locks so concurrent sends/triggers never interleave keystrokes.
    delivery_locks: Arc<Mutex<HashMap<String, Arc<Mutex<()>>>>>,
    login_env: Arc<OnceCell<Vec<(String, String)>>>,
    reaped_projects: Arc<std::sync::Mutex<HashSet<String>>>,
    /// Per-project broadcast channels for grid commands.
    grid_channels: Arc<Mutex<HashMap<String, tokio::sync::broadcast::Sender<GridCommand>>>>,
    /// Per-project broadcast channels for status push updates.
    status_channels: Arc<Mutex<HashMap<String, tokio::sync::broadcast::Sender<()>>>>,
    /// Projects that have been initialized or used — scheduler scans these.
    registered_projects: Arc<std::sync::Mutex<HashSet<String>>>,
    /// Agents with a resume in progress, so two resumes of one agent can never
    /// both spawn `claude --resume` on the same transcript.
    resuming: Arc<std::sync::Mutex<HashSet<String>>>,
    /// Pids of the agent processes daemon teardown is killing. Those agents
    /// must stay resumable, so their exits are not recorded as Broken; any
    /// other exit (including one that happened just before teardown) is.
    shutdown_killed: Arc<std::sync::Mutex<HashSet<u32>>>,
}

impl Default for Engine {
    fn default() -> Self {
        Self::new()
    }
}

/// Build a `ConfigReport` response from a loaded config.
/// Filters out the terminal agent (no launch args to configure).
fn config_to_report(
    cfg: &pu_core::types::Config,
    project_root: &Path,
    env: &[(String, String)],
) -> Response {
    let agents = cfg
        .agents
        .iter()
        .filter(|(name, _)| name.as_str() != "terminal")
        .map(|(name, ac)| {
            let resolved = pu_core::types::resolved_launch_args(name, ac.configured_launch_args());
            let resolved_command = if resolved.is_empty() {
                ac.command.clone()
            } else {
                format!("{} {}", ac.command, format_launch_args(&resolved))
            };
            AgentConfigInfo {
                id: Some(name.clone()),
                available: Some(catalog_executable_available(ac, name, project_root, env)),
                name: ac.name.clone(),
                command: ac.command.clone(),
                launch_args: ac.launch_args.clone(),
                resolved_launch_args: resolved,
                interactive: ac.interactive,
                project_launch_args: ac.launch_args.as_deref().map(format_launch_args),
                machine_launch_args: ac.machine_launch_args.as_deref().map(format_launch_args),
                policy_launch_args: ac.policy_launch_args.as_deref().map(format_launch_args),
                resolved_command: Some(resolved_command),
            }
        })
        .collect();
    Response::ConfigReport {
        terminal_available: Some(
            config::resolve_agent(cfg, "terminal")
                .is_some_and(|ac| catalog_executable_available(ac, "terminal", project_root, env)),
        ),
        default_agent: cfg.default_agent.clone(),
        agents,
    }
}

/// Root-cwd executable hint using Spawn parsing and login PATH, without running it.
/// A worktree-relative command or its arguments may still fail at launch.
fn catalog_executable_available(
    ac: &pu_core::types::AgentConfig,
    id: &str,
    cwd: &Path,
    env: &[(String, String)],
) -> bool {
    use std::os::unix::fs::PermissionsExt;
    let Ok((command, _)) = Engine::parse_agent_command(ac, id) else {
        return false;
    };
    let executable = |path: std::path::PathBuf| {
        std::fs::metadata(path).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
    };
    if command.contains('/') {
        let path = Path::new(&command);
        return executable(if path.is_absolute() {
            path.to_path_buf()
        } else {
            cwd.join(path)
        });
    }
    env.iter()
        .find(|(key, _)| key == "PATH")
        .is_some_and(|(_, path)| {
            path.split(':').any(|dir| {
                let dir = Path::new(dir);
                executable(if dir.is_absolute() {
                    dir.join(&command)
                } else {
                    cwd.join(dir).join(&command)
                })
            })
        })
}

impl Engine {
    pub fn new() -> Self {
        Self {
            sizes: Mutex::new(HashMap::new()),
            signal_states: Arc::new(std::sync::Mutex::new(HashMap::new())),
            native_sources: Arc::default(),
            project_registry_path: None,
            project_registry_updates: Arc::new(Mutex::new(())),
            start_time: Instant::now(),
            pty_host: NativePtyHost::new(),
            sessions: Arc::new(Mutex::new(HashMap::new())),
            pending_initial_inputs: Arc::new(Mutex::new(HashMap::new())),
            delivery_locks: Arc::new(Mutex::new(HashMap::new())),
            login_env: Arc::new(OnceCell::new()),
            reaped_projects: Arc::new(std::sync::Mutex::new(HashSet::new())),
            grid_channels: Arc::new(Mutex::new(HashMap::new())),
            status_channels: Arc::new(Mutex::new(HashMap::new())),
            registered_projects: Arc::new(std::sync::Mutex::new(HashSet::new())),
            resuming: Arc::new(std::sync::Mutex::new(HashSet::new())),
            shutdown_killed: Arc::new(std::sync::Mutex::new(HashSet::new())),
        }
    }

    /// Start a background task that periodically removes session handles for
    /// processes that have exited naturally, and cleans up broadcast channels
    /// with no subscribers. Without this, HashMap entries leak.
    pub fn start_session_reaper(self: &Arc<Self>) {
        let sessions = self.sessions.clone();
        let signal_states = self.signal_states.clone();
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
                        signal_states.lock().unwrap().remove(id);
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
        let shell = crate::os::login_shell();
        // Every spawn and resume waits on this one probe (it fills a OnceCell),
        // so a login shell that hangs on a prompt must not hang them all.
        let probe = tokio::process::Command::new(&shell)
            .args(["-li", "-c", "env -0"])
            .stdin(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true)
            .output();
        match tokio::time::timeout(Duration::from_secs(10), probe).await {
            Ok(Ok(output)) if output.status.success() => output
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

    pub(super) fn signal_snapshot(&self, id: &str) -> Option<pu_core::signals::SignalSnapshot> {
        self.signal_states.lock().ok()?.get(id)?.snapshot()
    }

    pub fn registered_projects(&self) -> Vec<String> {
        self.registered_projects
            .lock()
            .map(|p| {
                let mut roots: Vec<_> = p.iter().cloned().collect();
                roots.sort();
                roots
            })
            .unwrap_or_default()
    }

    /// Drain every active session and kill its process group (SIGTERM, brief wait, SIGKILL).
    /// Used by the managed-mode parent-died path so a force-quit of the macOS app reaps
    /// agents and their grandchildren (vitest/node workers, dev servers) before the
    /// daemon exits — `process::exit(0)` skips `Drop`, so this must run explicitly.
    ///
    /// Only processes still running when drained are recorded as killed by
    /// shutdown (and so left resumable); an agent that had already exited keeps
    /// its natural exit.
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

        // Mark only the agents still running at the moment they are signalled,
        // with no await in between: an agent that exits on its own before this
        // point (including during the snapshot above) keeps its natural exit.
        // The mark lands before the signal, so the exit watcher always sees it.
        {
            let mut killed = self
                .shutdown_killed
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            killed.extend(
                handles
                    .iter()
                    .filter(|h| h.exit_rx.borrow().is_none())
                    .map(|h| h.pid),
            );
        }

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
                if libc::kill(desc, 0) == 0 {
                    libc::kill(desc, libc::SIGKILL);
                }
            }
        }
    }

    pub async fn handle_request(&self, mut request: Request) -> Response {
        if let Err(response) = self.prepare_request(&mut request).await {
            return response;
        }

        self.handle_prepared_request(request).await
    }

    pub(crate) async fn handle_prepared_request(&self, request: Request) -> Response {
        if let Some(response) = self.handle_standalone_request(&request).await {
            return response;
        }
        match request {
            request @ (Request::ChannelRead { .. }
            | Request::ChannelSend { .. }
            | Request::ChannelEdit { .. }
            | Request::ChannelReact { .. }) => channel::handle(request).await,
            Request::Inventory {
                project_root,
                kind,
                state,
            } => Response::Inventory(self.inventory(project_root, kind, state).await),
            Request::ResolveAgent {
                agent_id,
                project_root,
            } => match self.resolve_agent(&agent_id, project_root).await {
                Ok((project_root, worktree_id)) => Response::AgentResolved {
                    agent_id,
                    project_root,
                    worktree_id,
                },
                Err(response) => response,
            },
            Request::Health => self.handle_health().await,
            Request::Init { project_root } => {
                let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
                if fs_ops::is_too_broad_project_root(
                    std::path::Path::new(&project_root),
                    home.as_deref(),
                ) {
                    return inventory::project_root_not_allowed(&project_root);
                }
                let response = self.handle_init(&project_root).await;
                if matches!(response, Response::InitResult { .. })
                    && let Err(e) = self.register_project(&project_root).await
                {
                    return e;
                }
                response
            }
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
            Request::GetGlobalAgentSettings => self.handle_global_agent_settings(None).await,
            Request::UpdateGlobalAgentSettings {
                codex_yolo,
                launch_args,
                agent_status_tracking,
            } => {
                self.handle_global_agent_settings(Some(config::GlobalAgentSettingsUpdate {
                    codex_yolo,
                    agent_status_tracking,
                    launch_args: launch_args.unwrap_or_default(),
                }))
                .await
            }
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
            Request::Read {
                project_root,
                agent_id,
                limit,
                since,
                session_id,
            } => {
                self.handle_read(
                    &project_root,
                    &agent_id,
                    limit,
                    since.as_deref(),
                    session_id.as_deref(),
                )
                .await
            }
            Request::Attach { agent_id, .. } => self.handle_attach(&agent_id).await,
            Request::Input {
                agent_id,
                data,
                submit,
                ..
            } => self.handle_input(&agent_id, &data, submit).await,
            Request::ClaimSize { .. } => Response::Error {
                code: "ATTACH_REQUIRED".into(),
                message: "claim_size requires an attachment".into(),
            },
            Request::Resize {
                agent_id,
                cols,
                rows,
            } => self.handle_legacy_resize(&agent_id, cols, rows).await,
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
            request @ (Request::FsList { .. }
            | Request::FsStat { .. }
            | Request::FsRead { .. }
            | Request::FsWrite { .. }
            | Request::FsDelete { .. }
            | Request::SubscribeFs { .. }
            | Request::GitExec { .. }) => self.handle_fs_request(request).await,
            request @ Request::AttachFile { .. } => self.handle_attach_file(request).await,
        }
    }

    async fn handle_health(&self) -> Response {
        let sessions = self.sessions.lock().await;
        Response::HealthReport {
            pid: std::process::id(),
            uptime_seconds: self.start_time.elapsed().as_secs(),
            protocol_version: PROTOCOL_VERSION,
            projects: self.registered_projects(),
            agent_count: sessions
                .values()
                .filter(|handle| handle.exit_rx.borrow().is_none())
                .count(),
        }
    }

    async fn handle_init(&self, project_root: &str) -> Response {
        let project_root = project_root.to_string();
        // Agents this daemon is running. Init runs on every project open and
        // every `pu init`, so reconcile must never touch these. Checked live,
        // not snapshotted: one can be spawned or resumed while reconcile runs.
        // Nothing holds the sessions lock while waiting on the manifest lock,
        // so taking it inside the manifest update cannot deadlock.
        let sessions = self.sessions.clone();
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
                    Self::reconcile_agents_on_init(&project_root, |id| {
                        sessions.blocking_lock().contains_key(id)
                    });
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

    /// Load the machine settings, or save `update` first when given.
    async fn handle_global_agent_settings(
        &self,
        update: Option<config::GlobalAgentSettingsUpdate>,
    ) -> Response {
        tokio::task::spawn_blocking(move || {
            let result = match update {
                Some(update) => config::save_global_agent_settings(update),
                None => config::load_global_agent_settings(),
            };
            match result {
                Ok(settings) => Response::GlobalAgentSettingsReport {
                    codex_yolo: settings.codex_yolo,
                    agent_status_tracking: settings.agent_status_tracking,
                    launch_args: settings
                        .launch_args
                        .iter()
                        .map(|(agent, args)| (agent.clone(), format_launch_args(&args.0)))
                        .collect(),
                },
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

    async fn handle_get_config(&self, project_root: &str) -> Response {
        let pr = project_root.to_string();
        let env = self.agent_env().await;
        tokio::task::spawn_blocking(move || {
            let root = Path::new(&pr);
            match config::load_config_strict(root) {
                Ok(cfg) => config_to_report(&cfg, root, &env),
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
        let env = self.agent_env().await;
        tokio::task::spawn_blocking(move || {
            let root = Path::new(&pr);
            match config::update_agent_config(root, &name, launch_args) {
                Ok(cfg) => config_to_report(&cfg, root, &env),
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

    #[tokio::test]
    async fn given_initialized_project_should_expose_it_in_global_health() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp
            .path()
            .canonicalize()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let engine = Engine::new();
        assert!(matches!(
            engine
                .handle_request(Request::Init {
                    project_root: root.clone()
                })
                .await,
            Response::InitResult { .. }
        ));
        match engine.handle_request(Request::Health).await {
            Response::HealthReport {
                projects,
                agent_count,
                ..
            } => {
                assert_eq!(projects, vec![root]);
                assert_eq!(agent_count, 0);
            }
            other => panic!("unexpected response: {other:?}"),
        }
    }

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

    fn agent_cfg(name: &str, launch_args: Option<Vec<String>>) -> pu_core::types::AgentConfig {
        pu_core::types::AgentConfig {
            name: name.into(),
            command: name.into(),
            prompt_flag: None,
            interactive: true,
            launch_args,
            machine_launch_args: None,
            policy_launch_args: None,
        }
    }

    #[test]
    fn given_yolo_policy_should_report_project_launch_args_unchanged() {
        // given — Codex YOLO rewrote the project's args into the policy slot
        let mut cfg = pu_core::types::Config::default();
        let codex = cfg.agents.get_mut("codex").unwrap();
        codex.launch_args = Some(vec!["--search".into()]);
        codex.policy_launch_args = Some(vec![
            "--search".into(),
            "--dangerously-bypass-approvals-and-sandbox".into(),
        ]);

        // when
        let Response::ConfigReport { agents, .. } = config_to_report(&cfg, Path::new("/"), &[])
        else {
            panic!("expected ConfigReport");
        };

        // then
        let codex = agents.iter().find(|a| a.name == "codex").unwrap();
        assert_eq!(codex.project_launch_args.as_deref(), Some("--search"));
        assert_eq!(
            codex.policy_launch_args.as_deref(),
            Some("--search --dangerously-bypass-approvals-and-sandbox")
        );
        assert_eq!(
            codex.resolved_launch_args,
            vec!["--search", "--dangerously-bypass-approvals-and-sandbox"]
        );
    }

    #[test]
    fn given_claude_build_resume_without_launch_args_should_launch_bare() {
        // given
        let engine = Engine::new();
        let agent_cfg = agent_cfg("claude", None);

        // when
        let (cmd, args, sid) = engine
            .build_resume_command("claude", &agent_cfg, Some("sess-123"))
            .unwrap();

        // then — no permission flags unless the user configured them
        assert_eq!(cmd, "claude");
        assert_eq!(args, vec!["--resume", "sess-123"]);
        assert_eq!(sid, Some("sess-123".to_string()));
    }

    #[test]
    fn given_claude_build_resume_with_machine_launch_args_should_include_them() {
        // given — the owner opted into auto mode for all projects
        let engine = Engine::new();
        let mut agent_cfg = agent_cfg("claude", None);
        agent_cfg.machine_launch_args = Some(vec!["--dangerously-skip-permissions".into()]);

        // when
        let (_, args, _) = engine
            .build_resume_command("claude", &agent_cfg, Some("sess-456"))
            .unwrap();

        // then
        assert_eq!(
            args,
            vec!["--dangerously-skip-permissions", "--resume", "sess-456"]
        );
    }

    #[test]
    fn given_claude_build_resume_with_empty_project_args_should_ignore_machine_args() {
        // given
        let engine = Engine::new();
        let mut agent_cfg = agent_cfg("claude", Some(vec![]));
        agent_cfg.machine_launch_args = Some(vec!["--dangerously-skip-permissions".into()]);

        // when
        let (_, args, _) = engine
            .build_resume_command("claude", &agent_cfg, Some("sess-456"))
            .unwrap();

        // then
        assert_eq!(args, vec!["--resume", "sess-456"]);
    }

    #[test]
    fn given_codex_build_resume_with_custom_launch_args_should_place_before_subcommand() {
        // given
        let engine = Engine::new();
        let agent_cfg = agent_cfg("codex", Some(vec!["--search".into()]));

        // when
        let (cmd, args, _) = engine
            .build_resume_command("codex", &agent_cfg, Some("session-exact"))
            .unwrap();

        // then — top-level flags must precede the subcommand
        assert_eq!(cmd, "codex");
        assert_eq!(
            args,
            vec!["--search", "--no-daemon", "resume", "session-exact"]
        );
    }

    #[test]
    fn given_terminal_build_resume_with_machine_launch_args_should_include_them() {
        // given — the owner configured a login shell for every project
        let engine = Engine::new();
        let mut agent_cfg = agent_cfg("terminal", None);
        agent_cfg.command = "shell".into();
        agent_cfg.machine_launch_args = Some(vec!["-l".into()]);

        // when
        let (cmd, args, sid) = engine
            .build_resume_command("terminal", &agent_cfg, None)
            .unwrap();

        // then — resume restarts the shell with the same options as spawn
        assert_eq!(cmd, crate::os::login_shell());
        assert_eq!(args, vec!["-l"]);
        assert_eq!(sid, None);
    }

    #[test]
    fn given_terminal_build_resume_with_project_launch_args_should_prefer_them() {
        // given
        let engine = Engine::new();
        let mut agent_cfg = agent_cfg("terminal", Some(vec!["-i".into()]));
        agent_cfg.command = "shell".into();
        agent_cfg.machine_launch_args = Some(vec!["-l".into()]);

        // when
        let (cmd, args, _) = engine
            .build_resume_command("terminal", &agent_cfg, None)
            .unwrap();

        // then
        assert_eq!(cmd, crate::os::login_shell());
        assert_eq!(args, vec!["-i"]);
    }

    #[test]
    fn given_codex_build_resume_without_launch_args_should_add_no_permission_flags() {
        // given
        let engine = Engine::new();
        let agent_cfg = agent_cfg("codex", None);

        // when
        let (cmd, args, _) = engine
            .build_resume_command("codex", &agent_cfg, Some("session-exact"))
            .unwrap();

        // then
        assert_eq!(cmd, "codex");
        assert_eq!(args, vec!["--no-daemon", "resume", "session-exact"]);
    }

    #[test]
    fn given_no_auto_should_ignore_project_and_machine_launch_args() {
        // given
        let mut project = agent_cfg("claude", Some(vec!["--verbose".into()]));
        let mut machine = agent_cfg("claude", None);
        machine.machine_launch_args = Some(vec!["--dangerously-skip-permissions".into()]);
        project.machine_launch_args = machine.machine_launch_args.clone();

        // then
        assert!(Engine::spawn_launch_args("claude", &project, true).is_empty());
        assert!(Engine::spawn_launch_args("claude", &machine, true).is_empty());
    }

    #[test]
    fn given_spawn_without_no_auto_should_use_configured_launch_args() {
        let mut cfg = agent_cfg("claude", None);
        assert!(Engine::spawn_launch_args("claude", &cfg, false).is_empty());
        cfg.machine_launch_args = Some(vec!["--model".into(), "opus".into()]);
        assert_eq!(
            Engine::spawn_launch_args("claude", &cfg, false),
            vec!["--model", "opus"]
        );
        cfg.launch_args = Some(vec!["--verbose".into()]);
        assert_eq!(
            Engine::spawn_launch_args("claude", &cfg, false),
            vec!["--verbose"]
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
            machine_launch_args: None,
            policy_launch_args: None,
        }
    }

    #[test]
    fn given_opencode_resume_should_use_exact_session() {
        let engine = Engine::new();
        let cfg = dummy_agent_cfg("opencode");
        let (cmd, args, _) = engine
            .build_resume_command("opencode", &cfg, Some("ses_exact"))
            .unwrap();
        assert_eq!(cmd, "opencode");
        assert!(args.contains(&"--session".to_string()));
        assert!(args.contains(&"ses_exact".to_string()));
        assert!(!args.contains(&"--continue".to_string()));
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

#[cfg(test)]
mod workspace_catalog_tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn agent(command: &str) -> pu_core::types::AgentConfig {
        pu_core::types::AgentConfig {
            name: "Friendly display label".into(),
            command: command.into(),
            prompt_flag: None,
            interactive: true,
            launch_args: None,
            machine_launch_args: None,
            policy_launch_args: None,
        }
    }

    #[test]
    fn given_custom_labels_and_missing_binary_should_report_canonical_id_and_actual_availability() {
        let root = tempfile::tempdir().unwrap();
        let mut cfg = pu_core::types::Config::default();
        cfg.agents.clear();
        cfg.agents.insert("canonical-id".into(), agent("/bin/sh"));
        cfg.agents
            .insert("missing-id".into(), agent("missing-purepoint-test-binary"));
        cfg.agents.insert("terminal".into(), agent("/bin/sh"));
        let Response::ConfigReport {
            agents,
            terminal_available,
            ..
        } = config_to_report(&cfg, root.path(), &[("PATH".into(), "/bin".into())])
        else {
            panic!("expected catalog");
        };
        assert_eq!(agents[0].id.as_deref(), Some("canonical-id"));
        assert_eq!(agents[0].name, "Friendly display label");
        assert_eq!(agents[0].available, Some(true));
        assert_eq!(agents[1].available, Some(false));
        assert_eq!(terminal_available, Some(true));
        assert_eq!(agents.len(), 2);
    }

    #[tokio::test]
    async fn given_scoped_terminal_spawn_should_publish_root_and_worktree_inventory_membership() {
        use pu_core::protocol::{InventoryKind, Request};
        let temp = tempfile::tempdir().unwrap();
        let project = temp.path().join("project");
        std::fs::create_dir(&project).unwrap();
        let project = project.canonicalize().unwrap();
        for args in [
            vec!["init", "-q"],
            vec![
                "-c",
                "user.name=Fixture",
                "-c",
                "user.email=fixture@example.invalid",
                "commit",
                "--allow-empty",
                "-qm",
                "fixture",
            ],
        ] {
            assert!(
                std::process::Command::new("git")
                    .args(args)
                    .current_dir(&project)
                    .status()
                    .unwrap()
                    .success()
            );
        }
        let engine = Engine::with_project_registry(temp.path().join("registry.json")).unwrap();
        engine
            .login_env
            .set(vec![("PATH".into(), "/usr/bin:/bin".into())])
            .unwrap();
        let root = project.to_string_lossy().into_owned();
        assert!(matches!(
            engine
                .handle_request(Request::Init {
                    project_root: root.clone()
                })
                .await,
            Response::InitResult { .. }
        ));
        let Response::CreateWorktreeResult { worktree_id } = engine
            .handle_request(Request::CreateWorktree {
                project_root: root.clone(),
                name: Some("terminal-fixture".into()),
                base: None,
            })
            .await
        else {
            panic!("worktree fixture creation failed");
        };
        let mut ids = vec![];
        for worktree in [None, Some(worktree_id.clone())] {
            let request: Request = serde_json::from_value(serde_json::json!({"type":"spawn", "project_root":root, "agent":"terminal", "prompt":"", "root":worktree.is_none(), "worktree":worktree, "no_trigger":true})).unwrap();
            let response = engine.handle_request(request).await;
            let Response::SpawnResult { agent_id, .. } = response else {
                engine.kill_all_sessions(Duration::from_millis(50)).await;
                panic!("terminal launch failed: {response:?}");
            };
            ids.push(agent_id);
        }
        let Response::Inventory(agents) = engine
            .handle_request(Request::Inventory {
                project_root: None,
                kind: InventoryKind::Agents,
                state: None,
            })
            .await
        else {
            panic!("expected inventory");
        };
        engine.kill_all_sessions(Duration::from_millis(50)).await;
        for (index, id) in ids.iter().enumerate() {
            let agent = agents.agents.iter().find(|a| &a.id == id).unwrap();
            assert_eq!(agent.project_root.as_deref(), Some(root.as_str()));
            assert_eq!(
                agent.worktree_id.as_deref(),
                if index == 0 {
                    None
                } else {
                    Some(worktree_id.as_str())
                }
            );
        }
        let Response::Inventory(worktrees) = engine
            .handle_request(Request::Inventory {
                project_root: None,
                kind: InventoryKind::Worktrees,
                state: None,
            })
            .await
        else {
            panic!("expected worktrees");
        };
        assert!(
            worktrees
                .worktrees
                .iter()
                .any(|w| w.id == worktree_id && w.agent_ids.contains(&ids[1]))
        );
    }

    #[test]
    fn given_login_path_and_root_relative_executables_should_probe_without_execution() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("tools")).unwrap();
        let executable = root.path().join("tools/worker");
        std::fs::write(&executable, "#!/bin/sh\nexit 99\n").unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
        let env = [(
            "PATH".into(),
            root.path().join("tools").display().to_string(),
        )];
        assert!(catalog_executable_available(
            &agent("worker --flag"),
            "custom",
            root.path(),
            &env
        ));
        assert!(catalog_executable_available(
            &agent("./tools/worker"),
            "custom",
            root.path(),
            &[]
        ));
        assert!(catalog_executable_available(
            &agent("worker"),
            "custom",
            &root.path().join("tools"),
            &[("PATH".into(), "".into())]
        ));
        assert!(!catalog_executable_available(
            &agent("./tools/worker"),
            "custom",
            &root.path().join("tools"),
            &[]
        ));
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert!(!catalog_executable_available(
            &agent("worker"),
            "custom",
            root.path(),
            &env
        ));
        assert!(!catalog_executable_available(
            &agent(""),
            "custom",
            root.path(),
            &env
        ));
    }
}
