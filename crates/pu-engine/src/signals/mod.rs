//! Observation-only, launch-scoped native adapters. No process or input ownership.
mod batch;
mod claude;
mod codex;
mod install;
mod normalize;
mod opencode;
mod qualification;

use pu_core::signals::*;
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// The launch's signal endpoint: a datagram socket at
/// `$TMP/pu-signal-<launch>/s` on Unix, a per-launch token-guarded named pipe
/// (`\\.\pipe\purepoint-<hash16>-signal-<launch>`, one frame per
/// connection) on Windows. Both hand the intake whole `{"token","payload"}`
/// frames through `recv`.
#[cfg(unix)]
pub(crate) type SignalSocket = tokio::net::UnixDatagram;
#[cfg(windows)]
pub(crate) type SignalSocket = pu_transport::signal::SignalListener;

pub(crate) type SignalStates = Arc<Mutex<HashMap<String, Arc<LaunchState>>>>;

/// One launch as Codex discovery sees it: its pid and cwd, and on Windows the
/// Codex home its environment names (discovery there scans that home by
/// creation time instead of following descriptors).
#[cfg(unix)]
pub(crate) type CodexKey = (u32, String);
#[cfg(windows)]
pub(crate) type CodexKey = (u32, String, Option<PathBuf>);

/// Read-only samplers live with this Engine, never in a global/service singleton.
pub(crate) struct NativeSources {
    codex: Arc<batch::Sampler<CodexKey, crate::read_source::CodexCandidate>>,
    opencode: opencode::Services,
}
impl Default for NativeSources {
    fn default() -> Self {
        Self {
            codex: Arc::new(batch::Sampler::new(Duration::from_secs(2))),
            opencode: opencode::Services::default(),
        }
    }
}

/// Existing native-read binding and telemetry share discovery, not ownership.
pub(crate) struct CodexReader {
    sampler: Arc<batch::Sampler<CodexKey, crate::read_source::CodexCandidate>>,
    id: String,
    startup_until: tokio::time::Instant,
    _registration: batch::Registration<CodexKey, crate::read_source::CodexCandidate>,
}
impl NativeSources {
    pub(crate) fn codex_reader(&self, launch: Option<&str>, key: CodexKey) -> Option<CodexReader> {
        let (id, registration) =
            match launch.and_then(|id| self.codex.attach(id, &key).map(|r| (id.to_owned(), r))) {
                Some(existing) => existing,
                None => {
                    let id = format!("read:{}", pu_core::id::session_id());
                    let registration = self.codex.register(id.clone(), key)?;
                    (id, registration)
                }
            };
        Some(CodexReader {
            sampler: self.codex.clone(),
            id,
            startup_until: tokio::time::Instant::now() + Duration::from_secs(2),
            _registration: registration,
        })
    }
}
impl CodexReader {
    pub(crate) async fn discover(
        &self,
    ) -> Result<crate::read_source::Binding, crate::read_source::SourceError> {
        self.discover_with(refresh_codex).await
    }
    async fn discover_with<F, Fut>(
        &self,
        mut refresh: F,
    ) -> Result<crate::read_source::Binding, crate::read_source::SourceError>
    where
        F: FnMut(Vec<(String, CodexKey)>) -> Fut,
        Fut: std::future::Future<
                Output = HashMap<String, Option<crate::read_source::CodexCandidate>>,
            >,
    {
        // Reuse a positive view immediately; forcing another scan could lose
        // an already observed descriptor when a short-lived process exits.
        if let Some(candidate) = self.sampler.sample(&self.id, false, &mut refresh).await {
            return candidate.into_binding();
        }
        // Only burst during initial launch. A blank session can remain unbound
        // indefinitely: afterward share the ordinary two-second idle cadence.
        // Real native Working edges still refresh through the observer itself.
        if tokio::time::Instant::now() >= self.startup_until {
            return Err(crate::read_source::SourceError::IdentityUnavailable);
        }
        // A startup reader must not sit on a two-second cached miss.
        // Urgent requests still coalesce at the sampler's shared 250ms floor.
        self.sampler
            .sample(&self.id, true, refresh)
            .await
            .ok_or(crate::read_source::SourceError::IdentityUnavailable)?
            .into_binding()
    }
}
async fn refresh_codex(
    roots: Vec<(String, CodexKey)>,
) -> HashMap<String, Option<crate::read_source::CodexCandidate>> {
    #[cfg(unix)]
    let roots = roots
        .into_iter()
        .map(|(id, (pid, cwd))| (id, pid, cwd))
        .collect();
    #[cfg(windows)]
    let roots = roots
        .into_iter()
        .map(|(id, (pid, cwd, home))| (id, pid, cwd, home))
        .collect();
    tokio::time::timeout(
        Duration::from_secs(2),
        crate::read_source::discover_codex_roots(roots),
    )
    .await
    .unwrap_or_default()
}

// Native image/PDF tool results can exceed 1MiB in one JSONL record. Keep the
// per-poll I/O budget separate from this finite record-assembly limit.
const MAX_NATIVE_RECORD_BYTES: usize = 32 * 1024 * 1024;

// A JSONL append may be incomplete while its already parsed prefix remains
// valid. Track the greatest observed length so even an unread-tail truncation
// invalidates that prefix; never follow replacement links or another inode.
#[cfg(windows)]
fn native_prefix_length(
    path: &Path,
    inode: Option<(u64, u64)>,
    offset: u64,
    observed_length: u64,
) -> Option<u64> {
    use pu_core::private_fs as pfs;
    // Windows has no path-based file index: open the path itself, never a
    // reparse point's target, and compare the handle's identity.
    let file = pfs::open_no_follow(path).ok()?;
    let m = file.metadata().ok()?;
    (m.is_file()
        && pfs::owned_by_current_user(&file).ok()?
        && pfs::file_identity(&file).ok() == inode
        && m.len() >= offset
        && m.len() >= observed_length)
        .then_some(m.len())
}

#[cfg(unix)]
fn native_prefix_length(
    path: &Path,
    inode: Option<(u64, u64)>,
    offset: u64,
    observed_length: u64,
) -> Option<u64> {
    use std::os::unix::fs::MetadataExt;
    let m = std::fs::symlink_metadata(path).ok()?;
    (m.is_file()
        && m.uid() == unsafe { libc::geteuid() }
        && Some((m.dev(), m.ino())) == inode
        && m.len() >= offset
        && m.len() >= observed_length)
        .then_some(m.len())
}

struct NativeFileBoundary {
    offset: u64,
    inode: (u64, u64),
}

fn native_file_boundary(path: &Path) -> Option<NativeFileBoundary> {
    use pu_core::private_fs as pfs;
    use std::io::{Read, Seek, SeekFrom};
    let mut file = pfs::open_no_follow(path).ok()?;
    let before = file.metadata().ok()?;
    if !before.is_file() || !pfs::owned_by_current_user(&file).ok()? || before.len() == 0 {
        return None;
    }
    file.seek(SeekFrom::End(-1)).ok()?;
    let mut last = [0];
    file.read_exact(&mut last).ok()?;
    if last != [b'\n'] || file.metadata().ok()?.len() != before.len() {
        return None;
    }
    Some(NativeFileBoundary {
        offset: before.len(),
        inode: pfs::file_identity(&file).ok()?,
    })
}

/// Unix: owner-only mode exactly 0600 and owned by the euid. Windows: owned
/// by the user with a private DACL (`pu_core::private_fs::check_private`).
fn exact_private_file(file: &std::fs::File, label: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let _ = label;
        file.metadata().is_ok_and(|meta| {
            meta.mode() & 0o7777 == 0o600 && meta.uid() == unsafe { libc::geteuid() }
        })
    }
    #[cfg(windows)]
    {
        pu_core::private_fs::check_private_file(file, label).is_ok()
    }
}

pub(crate) struct LaunchState {
    reducer: Mutex<Option<SignalReducer>>,
    identity: Mutex<SignalIdentity>,
    native_snapshot: Mutex<Option<SignalSnapshot>>,
    macos_qualification: Option<qualification::Grant>,
    // Dropping the last state removes only exact owned artifacts.
    _artifacts: install::Artifacts,
}

impl LaunchState {
    pub(crate) fn launch_id(&self) -> String {
        self.identity.lock().unwrap().launch_id.clone()
    }
    pub(crate) fn snapshot(&self) -> Option<SignalSnapshot> {
        let native = self.native_snapshot.lock().ok()?.clone();
        let is_native = native.is_some();
        let mut snapshot = native.or_else(|| {
            self.reducer
                .lock()
                .ok()?
                .as_ref()
                .map(|r| r.snapshot(now_ms()))
        })?;
        let authorized = self.macos_qualification.as_ref().is_some_and(|grant| {
            grant.current_now()
                && snapshot
                    .evidence
                    .as_ref()
                    .is_some_and(|evidence| grant.matches(evidence))
        });
        let current = snapshot.is_current(now_ms());
        for request in &mut snapshot.pending_attention {
            if is_native {
                request.actionability.macos =
                    authorized && current && request.confidence == SignalConfidence::Authoritative;
                request.actionability.ios = false;
            } else if !authorized {
                request.actionability.macos = false;
            }
        }
        Some(snapshot)
    }
}

pub(crate) struct Prepared {
    socket: SignalSocket,
    token: String,
    tool: String,
    version: String,
    state: Arc<LaunchState>,
    observer: Option<NativeObserver>,
    output: Option<Arc<crate::output_buffer::OutputBuffer>>,
}

enum NativeObserver {
    OpenCode(opencode::Observer),
    Claude(claude::Observer),
    Codex(codex::Observer),
}

/// Publish semantic changes immediately while renewing a subscribed consumer's
/// 30-second observation lease with a bounded heartbeat. Comparing raw snapshots
/// would broadcast every poll; ignoring timestamps forever would make live UI stale.
#[derive(Default)]
struct NativePublication {
    previous: Option<Option<SignalSnapshot>>,
    sent_at: Option<tokio::time::Instant>,
}

impl NativePublication {
    fn should_publish(
        &mut self,
        mut snapshot: Option<SignalSnapshot>,
        now: tokio::time::Instant,
    ) -> bool {
        let current = snapshot
            .as_ref()
            .is_some_and(|s| s.observation.state == ObservationState::Current);
        if let Some(snapshot) = snapshot.as_mut() {
            snapshot.observation.last_received_at_ms = None;
            snapshot.observation.expires_at_ms = None;
        }
        let changed = self.previous.as_ref() != Some(&snapshot);
        let heartbeat = current
            && self
                .sent_at
                .is_some_and(|last| now.saturating_duration_since(last) >= Duration::from_secs(10));
        if !changed && !heartbeat {
            return false;
        }
        self.previous = Some(snapshot);
        self.sent_at = Some(now);
        true
    }
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}
fn limits() -> SignalLimits {
    SignalLimits {
        freshness_ms: 30_000,
        max_pending: 64,
        max_event_ids: 512,
    }
}

fn helper(env: &[(String, String)]) -> Option<PathBuf> {
    let name = format!("pu{}", std::env::consts::EXE_SUFFIX);
    let sibling = std::env::current_exe().ok()?.with_file_name(&name);
    if sibling.is_file() {
        return Some(sibling);
    }
    let path = env.iter().rev().find(|(k, _)| k == "PATH")?.1.clone();
    std::env::split_paths(&path)
        .map(|p| p.join(&name))
        .find(|p| p.is_absolute() && p.is_file())
}

/// Whether the user has status tracking on (the default). An unreadable
/// settings file leaves it on, matching a missing one.
pub(crate) async fn tracking_enabled() -> bool {
    tokio::task::spawn_blocking(|| {
        pu_core::config::load_global_agent_settings().map_or(true, |s| s.agent_status_tracking)
    })
    .await
    .unwrap_or(true)
}

/// Fresh launches never inherit another launch's signal capabilities.
pub(crate) fn strip_inherited(env: &mut Vec<(String, String)>) {
    env.retain(|(key, _)| !key.starts_with("PU_SIGNAL_"));
}

/// Add launch-only observation artifacts. Failure never prevents the native launch.
/// OpenCode observes its existing service without changing launch options.
/// Claude reads its native interactive registry and transcript.
/// Codex reads the existing PTY protocol and its exact owned rollout.
/// Gemini/Cursor still require production source connections.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn prepare(
    project: &str,
    cwd: &str,
    agent: &str,
    worktree: Option<String>,
    tool: &str,
    session: Option<&str>,
    resuming: bool,
    command: &str,
    args: &mut Vec<String>,
    env: &mut Vec<(String, String)>,
) -> Option<Prepared> {
    strip_inherited(env);
    if tool == "opencode" && Path::new(command).file_name()?.to_str()? == tool {
        let mut prepared =
            opencode::prepare_launch(project, cwd, agent, worktree, session?, args, env, resuming)
                .await?;
        Arc::get_mut(&mut prepared.state)?.macos_qualification =
            qualification::load(project, tool, "1", args).await;
        return Some(prepared);
    }
    if tool == "codex" && Path::new(command).file_name()?.to_str()? == tool {
        let mut prepared =
            codex::prepare_launch(project, agent, worktree, session, cwd, env).await?;
        Arc::get_mut(&mut prepared.state)?.macos_qualification =
            qualification::load(project, tool, "2", args).await;
        return Some(prepared);
    }
    if tool == "claude" && Path::new(command).file_name()?.to_str()? == tool {
        let mut prepared =
            claude::prepare_launch(project, cwd, agent, worktree, session?, env, resuming).await?;
        Arc::get_mut(&mut prepared.state)?.macos_qualification =
            qualification::load(project, tool, "2", args).await;
        return Some(prepared);
    }
    if tool != "pi" || Path::new(command).file_name()?.to_str()? != tool {
        return None;
    }
    let helper = helper(env)?;
    let launch = pu_core::id::session_id();
    let mut artifacts = install::Artifacts::new(Path::new(project), &launch).ok()?;
    let home = artifacts.socket_directory.join("home");
    std::fs::create_dir(&home).ok()?;
    let path = env
        .iter()
        .rev()
        .find(|(k, _)| k == "PATH")
        .map(|(_, v)| v.as_str())
        .unwrap_or("");
    // Verify CLI compatibility and warm OS executable validation before native
    // hooks can run. Never fall back to an older CLI's normal routing path.
    let mut helper_probe = tokio::process::Command::new(&helper);
    helper_probe
        .arg("__agent-signal")
        .env_clear()
        .env("HOME", &home)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true);
    #[cfg(windows)]
    helper_probe.creation_flags(crate::git::HELPER_CREATION_FLAGS);
    let helper_probe = helper_probe.status();
    if !tokio::time::timeout(Duration::from_millis(500), helper_probe)
        .await
        .ok()?
        .ok()?
        .success()
    {
        return None;
    }
    // Record the running version for evidence only; any release is observed.
    let probe = async {
        use tokio::io::AsyncReadExt;
        let mut child = tokio::process::Command::new(command);
        #[cfg(windows)]
        child.creation_flags(crate::git::HELPER_CREATION_FLAGS);
        let mut child = child
            .arg("--version")
            .env_clear()
            .env("HOME", &home)
            .env("PATH", path)
            .env("XDG_CONFIG_HOME", &home)
            .env("PI_CODING_AGENT_DIR", &home)
            .current_dir(&home)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .ok()?;
        let mut bytes = Vec::new();
        child
            .stdout
            .take()?
            .take(4097)
            .read_to_end(&mut bytes)
            .await
            .ok()?;
        if bytes.len() > 4096 || !child.wait().await.ok()?.success() {
            return None;
        }
        String::from_utf8_lossy(&bytes)
            .split_whitespace()
            .find(|v| v.starts_with(|c: char| c.is_ascii_digit()) && v.len() <= 64)
            .map(str::to_owned)
    };
    let version = tokio::time::timeout(Duration::from_millis(500), probe)
        .await
        .ok()
        .flatten()
        .unwrap_or_else(|| "unknown".into());
    let _ = pu_core::fs_retry::remove_dir(&home);
    let socket = artifacts.bind_signal().ok()?;
    let token = format!("{}{}", pu_core::id::session_id(), pu_core::id::session_id());
    let identity = SignalIdentity {
        agent_id: agent.into(),
        worktree_id: worktree,
        launch_id: launch,
        generation: 1,
        native_session_id: session.unwrap_or("").into(),
    };
    let reducer = if session.is_some() {
        Some(SignalReducer::new(identity.clone(), limits()).ok()?)
    } else {
        None
    };
    let macos_qualification = qualification::load(project, tool, "1", args).await;
    let p = artifacts
        .write(
            "pi.mjs",
            include_bytes!("../../assets/agent-signals/pi.mjs").to_vec(),
        )
        .ok()?;
    let config = vec!["--extension".into(), p.to_string_lossy().into_owned()];
    args.extend(config);
    env.extend([
        ("PU_SIGNAL_TOKEN".into(), token.clone()),
        ("PU_SIGNAL_SOCKET".into(), artifacts.signal_address.clone()),
        (
            "PU_SIGNAL_HELPER".into(),
            helper.to_string_lossy().into_owned(),
        ),
    ]);
    Some(Prepared {
        socket,
        token,
        tool: tool.into(),
        version,
        state: Arc::new(LaunchState {
            reducer: Mutex::new(reducer),
            identity: Mutex::new(identity),
            native_snapshot: Mutex::new(None),
            macos_qualification,
            _artifacts: artifacts,
        }),
        observer: None,
        output: None,
    })
}

impl Prepared {
    pub(crate) fn with_output(mut self, output: Arc<crate::output_buffer::OutputBuffer>) -> Self {
        self.output = Some(output);
        self
    }
    #[cfg(test)]
    pub(crate) fn activate(
        self,
        states: &SignalStates,
        project: &str,
        cwd: &str,
        pid: u32,
        exit: tokio::sync::watch::Receiver<Option<i32>>,
        channels: Arc<tokio::sync::Mutex<HashMap<String, tokio::sync::broadcast::Sender<()>>>>,
    ) {
        self.activate_with_sources(states, project, cwd, pid, exit, channels, Arc::default());
    }
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn activate_with_sources(
        self,
        states: &SignalStates,
        project: &str,
        cwd: &str,
        pid: u32,
        mut exit: tokio::sync::watch::Receiver<Option<i32>>,
        channels: Arc<tokio::sync::Mutex<HashMap<String, tokio::sync::broadcast::Sender<()>>>>,
        sources: Arc<NativeSources>,
    ) {
        let id = self.state.identity.lock().unwrap().agent_id.clone();
        states.lock().unwrap().insert(id, self.state.clone());
        let weak = Arc::downgrade(&self.state);
        let project = project.to_string();
        if let Some(observer) = self.observer {
            match observer {
                NativeObserver::OpenCode(observer) => observer.activate(
                    weak,
                    project,
                    cwd.to_owned(),
                    exit,
                    channels,
                    &sources.opencode,
                ),
                NativeObserver::Codex(observer) => {
                    if let Some(output) = self.output {
                        observer.activate(
                            weak,
                            project,
                            cwd.to_owned(),
                            pid,
                            exit,
                            channels,
                            output,
                            sources.codex.clone(),
                        );
                    }
                }
                NativeObserver::Claude(observer) => {
                    observer.activate(weak, project, cwd.to_owned(), pid, exit, channels)
                }
            }
            return;
        }
        let Self {
            socket,
            token,
            tool,
            version,
            state: _,
            observer: _,
            output: _,
        } = self;
        tokio::spawn(async move {
            let mut data = vec![0_u8; 256 * 1024 + 1];
            let mut tick = tokio::time::interval(Duration::from_secs(1));
            let mut intake = Intake {
                tool,
                version,
                pid,
                sequence: 0,
                previous_native: None,
            };
            let mut previous_snapshot = None;
            loop {
                let Some(state) = weak.upgrade() else { break };
                let mut dead = false;
                tokio::select! {
                    result=socket.recv(&mut data)=> {
                        let Ok(size)=result else {break};
                        if size>256*1024 { continue; }
                        intake.receive(&state, &data[..size], &token);
                    },
                    result=exit.changed()=> { if result.is_err()||exit.borrow().is_some() {dead=true;} },
                    _=tick.tick()=> { if exit.borrow().is_some() {dead=true;} },
                }
                if dead && let Some(r) = state.reducer.lock().unwrap().as_mut() {
                    r.invalidate(ObservationState::ProcessDead);
                }
                let snapshot = state.snapshot();
                if snapshot != previous_snapshot {
                    previous_snapshot = snapshot;
                    if let Some(tx) = channels.lock().await.get(&project) {
                        let _ = tx.send(());
                    }
                }
                if dead {
                    break;
                }
                // Release state across the next wait so replacement permits cleanup.
                drop(state);
            }
        });
    }
}

struct Intake {
    tool: String,
    version: String,
    pid: u32,
    sequence: u64,
    previous_native: Option<u64>,
}
impl Intake {
    fn receive(&mut self, state: &LaunchState, data: &[u8], token: &str) {
        let Ok(frame) = serde_json::from_slice::<Value>(data) else {
            return;
        };
        let Some(received) = frame["token"].as_str() else {
            return;
        };
        if !token_matches(token.as_bytes(), received.as_bytes()) {
            return;
        }
        let payload = &frame["payload"];
        let Some(session) = normalize::session(&self.tool, payload) else {
            return;
        };
        if self.tool == "pi"
            && payload.get("turn_id").is_some_and(|id| {
                !id.is_null()
                    && !id
                        .as_str()
                        .is_some_and(crate::read_source::valid_session_id)
            })
        {
            return;
        }
        let pi_sequence = if self.tool == "pi" {
            if payload["native_pid"].as_u64() != Some(u64::from(self.pid)) {
                return;
            }
            let Some(sequence) = payload["sequence"]
                .as_u64()
                .filter(|n| (1..=(1_u64 << 53) - 1).contains(n))
            else {
                return;
            };
            let Some(time) = payload["occurred_at_ms"].as_i64() else {
                return;
            };
            let now = now_ms();
            if time > now || now.saturating_sub(time) >= i64::from(limits().freshness_ms) {
                return;
            }
            Some(sequence)
        } else {
            None
        };
        let mut identity = state.identity.lock().unwrap();
        let mut reducer = state.reducer.lock().unwrap();
        if self.tool == "pi" && reducer.is_none() {
            if payload["type"] != "observer_bound"
                || pi_sequence != Some(1)
                || !crate::read_source::valid_session_id(session)
            {
                return;
            }
            identity.native_session_id = session.into();
            *reducer = SignalReducer::new(identity.clone(), limits()).ok();
            self.previous_native = Some(1);
            return;
        }
        if identity.native_session_id != session {
            return;
        }
        let Some(reducer) = reducer.as_mut() else {
            return;
        };
        if self.tool == "pi" {
            // Unknown or malformed native events cannot poison the sequence watermark.
            if normalize::kind(&self.tool, payload).is_none() {
                return;
            }
            let native = pi_sequence.unwrap();
            if self.previous_native.is_some_and(|old| native <= old) {
                return;
            }
            if self.previous_native.is_none_or(|old| native != old + 1) {
                reducer.invalidate(ObservationState::Lost);
            }
        }
        if let Some((mut kind, confidence)) = normalize::kind(&self.tool, payload) {
            self.sequence += 1;
            let event_id = if self.tool == "claude" {
                format!(
                    "{}:{}",
                    identity.launch_id,
                    normalize::claude_event_id(payload)
                )
            } else if self.tool == "pi" {
                format!("{}-{}", identity.launch_id, pi_sequence.unwrap())
            } else {
                payload["id"]
                    .as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| {
                        format!(
                            "{}-{}",
                            identity.launch_id,
                            pi_sequence.unwrap_or(self.sequence)
                        )
                    })
            };
            let turn_id = if self.tool == "claude" {
                normalize::native_turn(payload)
            } else {
                payload["turn_id"].as_str().map(str::to_owned)
            };
            if let SignalEventKind::AttentionPending { request } = &mut kind {
                request.event_id = event_id.clone();
                request.turn_id = turn_id.clone();
                request.actionability.macos = state
                    .macos_qualification
                    .as_ref()
                    .is_some_and(|grant| grant.current_now());
                request.actionability.ios = false;
            }
            let event = SignalEvent {
                schema_version: 1,
                tool: self.tool.clone(),
                tool_version: self.version.clone(),
                adapter_version: "1".into(),
                identity: identity.clone(),
                parent_session_id: None,
                turn_id,
                event_id,
                sequence: self.sequence,
                occurred_at_ms: payload["occurred_at_ms"].as_i64().unwrap_or_else(now_ms),
                confidence,
                kind,
            };
            let result = reducer.apply(event, now_ms());
            if result != ApplyResult::Invalid && self.tool == "pi" {
                self.previous_native = pi_sequence;
            }
        }
    }
}

fn token_matches(expected: &[u8], received: &[u8]) -> bool {
    if expected.len() != received.len() {
        return false;
    }
    expected
        .iter()
        .zip(received)
        .fold(0_u8, |diff, (a, b)| std::hint::black_box(diff | (a ^ b)))
        == 0
}

/// Cross-crate test support: a hook-fed launch intake bound exactly like a
/// production launch's, so `pu`'s helper can be driven end to end.
#[doc(hidden)]
pub mod testing {
    use super::*;

    pub struct HookIntake {
        /// `PU_SIGNAL_SOCKET` for the helper.
        pub address: String,
        /// `PU_SIGNAL_TOKEN` for the helper.
        pub token: String,
        states: SignalStates,
        _exit: tokio::sync::watch::Sender<Option<i32>>,
        _project: tempfile::TempDir,
    }

    impl HookIntake {
        /// Bind and activate a `tool` launch (`"claude"` or `"pi"`) owned by
        /// `pid`. Call inside a multi-threaded Tokio runtime.
        pub fn start(tool: &str, session: Option<&str>, pid: u32) -> std::io::Result<Self> {
            let project = tempfile::tempdir()?;
            let launch = pu_core::id::session_id();
            let artifacts = install::Artifacts::new(project.path(), &launch)?;
            let socket = artifacts.bind_signal()?;
            let address = artifacts.signal_address.clone();
            let token = format!("{}{}", pu_core::id::session_id(), pu_core::id::session_id());
            let identity = SignalIdentity {
                agent_id: "ag-hook".into(),
                worktree_id: None,
                launch_id: launch,
                generation: 1,
                native_session_id: session.unwrap_or("").into(),
            };
            let reducer = session
                .map(|_| SignalReducer::new(identity.clone(), limits()))
                .transpose()
                .map_err(|e| std::io::Error::other(format!("{e:?}")))?;
            let prepared = Prepared {
                socket,
                token: token.clone(),
                tool: tool.into(),
                version: "test".into(),
                state: Arc::new(LaunchState {
                    reducer: Mutex::new(reducer),
                    identity: Mutex::new(identity),
                    native_snapshot: Mutex::new(None),
                    macos_qualification: None,
                    _artifacts: artifacts,
                }),
                observer: None,
                output: None,
            };
            let states = SignalStates::default();
            let (exit, rx) = tokio::sync::watch::channel(None);
            prepared.activate_with_sources(
                &states,
                "project",
                "/owned",
                pid,
                rx,
                Arc::default(),
                Arc::default(),
            );
            Ok(Self {
                address,
                token,
                states,
                _exit: exit,
                _project: project,
            })
        }

        /// The launch's current observed state.
        pub fn snapshot(&self) -> Option<SignalSnapshot> {
            self.states.lock().unwrap()["ag-hook"].snapshot()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn fixture(tool: &str, session: Option<&str>) -> (Prepared, tempfile::TempDir) {
        let tmp = tempfile::tempdir().unwrap();
        let launch = pu_core::id::session_id();
        let artifacts = install::Artifacts::new(tmp.path(), &launch).unwrap();
        let socket = artifacts.bind_signal().unwrap();
        let identity = SignalIdentity {
            agent_id: "ag-test".into(),
            worktree_id: Some("wt-test".into()),
            launch_id: launch,
            generation: 1,
            native_session_id: session.unwrap_or("").into(),
        };
        let reducer = session.map(|_| SignalReducer::new(identity.clone(), limits()).unwrap());
        (
            Prepared {
                socket,
                token: "private-fixture-token".into(),
                tool: tool.into(),
                version: "fixture".into(),
                state: Arc::new(LaunchState {
                    reducer: Mutex::new(reducer),
                    identity: Mutex::new(identity),
                    native_snapshot: Mutex::new(None),
                    macos_qualification: None,
                    _artifacts: artifacts,
                }),
                observer: None,
                output: None,
            },
            tmp,
        )
    }
    async fn settle() {
        tokio::time::sleep(Duration::from_millis(if cfg!(windows) { 100 } else { 30 })).await;
    }
    /// Deliver one frame to the launch endpoint the way the helper does: a
    /// datagram on Unix, one pipe connection on Windows.
    fn send(address: &str, token: &str, mut payload: Value) {
        payload
            .as_object_mut()
            .unwrap()
            .entry("occurred_at_ms")
            .or_insert_with(|| now_ms().into());
        let frame = serde_json::to_vec(&json!({"token":token,"payload":payload})).unwrap();
        #[cfg(unix)]
        std::os::unix::net::UnixDatagram::unbound()
            .unwrap()
            .send_to(&frame, address)
            .unwrap();
        #[cfg(windows)]
        pu_transport::signal::send_frame(
            address,
            &frame,
            std::time::Instant::now() + Duration::from_secs(2),
        )
        .unwrap();
    }
    #[tokio::test(start_paused = true)]
    async fn given_long_idle_unbound_reader_should_not_force_scans_after_startup() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let sources = NativeSources::default();
        #[cfg(unix)]
        let key = (123, "/owned".to_owned());
        #[cfg(windows)]
        let key = (123, "/owned".to_owned(), None);
        let reader = sources.codex_reader(None, key).unwrap();
        tokio::time::advance(Duration::from_secs(3)).await;
        let calls = AtomicUsize::new(0);
        for _ in 0..100 {
            assert!(
                reader
                    .discover_with(|_| {
                        calls.fetch_add(1, Ordering::SeqCst);
                        async { HashMap::new() }
                    })
                    .await
                    .is_err()
            );
        }
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "Idle probes must reuse the bounded shared miss, not force250ms scans forever"
        );
        // The native observer can still force a real Working transition, and
        // the idle reader must immediately reuse that successful source view.
        sources
            .codex
            .sample(&reader.id, true, |roots| async {
                calls.fetch_add(1, Ordering::SeqCst);
                roots
                    .into_iter()
                    .map(|(id, _)| {
                        (
                            id,
                            Some(crate::read_source::CodexCandidate::Bound(
                                crate::read_source::Binding {
                                    version: 1,
                                    runtime: pu_core::read::Runtime::Codex,
                                    session_id: "started-root".into(),
                                    path: "/owned/rollout.jsonl".into(),
                                    cwd: "/owned".into(),
                                },
                            )),
                        )
                    })
                    .collect()
            })
            .await;
        let binding = reader
            .discover_with(|_| async { panic!("Positive cache must not force another scan") })
            .await
            .unwrap();
        assert_eq!(binding.session_id, "started-root");
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    // Ownership by held descriptor (`exec 3<`) is Unix; the Windows
    // counterpart below proves the same retry with creation-time ownership.
    #[cfg(unix)]
    #[tokio::test]
    async fn given_unbound_reader_with_cached_miss_should_retry_newly_opened_owned_rollout() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("rollout-late.jsonl");
        std::fs::write(
            &path,
            r#"{"type":"session_meta","payload":{"source":"cli","id":"late-root","cwd":"/owned"}}
"#,
        )
        .unwrap();
        let mut child = tokio::process::Command::new("sh")
            .args([
                "-c",
                "read signal; exec 3< \"$1\"; printf x; exec sleep 30",
                "probe",
            ])
            .arg(path)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let sources = NativeSources::default();
        let reader = sources
            .codex_reader(None, (child.id().unwrap(), "/owned".to_owned()))
            .unwrap();
        assert!(reader.discover().await.is_err(), "No rollout is held yet");
        child
            .stdin
            .as_mut()
            .unwrap()
            .write_all(b"open\n")
            .await
            .unwrap();
        let mut ready = [0];
        child
            .stdout
            .as_mut()
            .unwrap()
            .read_exact(&mut ready)
            .await
            .unwrap();
        let result = tokio::time::timeout(Duration::from_secs(1), reader.discover()).await;
        child.kill().await.unwrap();
        child.wait().await.unwrap();
        assert_eq!(result.unwrap().unwrap().session_id, "late-root");
    }

    /// Windows: a rollout created under the launch's Codex home after a
    /// cached miss is found by the next discovery.
    #[cfg(windows)]
    #[tokio::test]
    async fn given_unbound_reader_with_cached_miss_should_retry_newly_created_owned_rollout() {
        let home = tempfile::tempdir().unwrap();
        let mut child = tokio::process::Command::new("ping")
            .args(["-n", "30", "127.0.0.1"])
            .stdout(std::process::Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let sources = NativeSources::default();
        let reader = sources
            .codex_reader(
                None,
                (
                    child.id().unwrap(),
                    "/owned".to_owned(),
                    Some(home.path().to_path_buf()),
                ),
            )
            .unwrap();
        assert!(reader.discover().await.is_err(), "No rollout exists yet");
        let today = chrono::Local::now();
        let day = home
            .path()
            .join("sessions")
            .join(today.format("%Y").to_string())
            .join(today.format("%m").to_string())
            .join(today.format("%d").to_string());
        std::fs::create_dir_all(&day).unwrap();
        std::fs::write(
            day.join("rollout-late.jsonl"),
            r#"{"type":"session_meta","payload":{"source":"cli","id":"late-root","cwd":"/owned"}}
"#,
        )
        .unwrap();
        let result = tokio::time::timeout(Duration::from_secs(1), reader.discover()).await;
        child.kill().await.unwrap();
        child.wait().await.unwrap();
        assert_eq!(result.unwrap().unwrap().session_id, "late-root");
    }

    #[tokio::test]
    async fn native_publication_preserves_semantics_and_renews_live_consumer_leases() {
        let (prepared, _root) = fixture("claude", Some("native"));
        let mut snapshot = prepared
            .state
            .reducer
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .snapshot(1000);
        snapshot.observation.state = ObservationState::Current;
        snapshot.observation.last_received_at_ms = Some(1000);
        snapshot.observation.expires_at_ms = Some(31000);
        let mut publication = NativePublication::default();
        let start = tokio::time::Instant::now();
        assert!(publication.should_publish(Some(snapshot.clone()), start));
        for second in 1..10 {
            snapshot.observation.last_received_at_ms = Some(1000 + second * 1000);
            snapshot.observation.expires_at_ms = Some(31000 + second * 1000);
            assert!(!publication.should_publish(
                Some(snapshot.clone()),
                start + Duration::from_secs(second as u64)
            ));
        }
        assert!(
            publication.should_publish(Some(snapshot.clone()), start + Duration::from_secs(10))
        );
        snapshot.observation.state = ObservationState::Disconnected;
        assert!(
            publication
                .should_publish(Some(snapshot.clone()), start + Duration::from_millis(10001))
        );
        assert!(
            !publication.should_publish(Some(snapshot.clone()), start + Duration::from_secs(30))
        );
        snapshot.observation.state = ObservationState::Current;
        snapshot.pending_attention.push(PendingAttention {
            request_id: "request".into(),
            event_id: "event".into(),
            turn_id: Some("turn".into()),
            kind: AttentionKind::Question,
            confidence: SignalConfidence::Authoritative,
            actionability: AttentionActionability::default(),
        });
        assert!(
            publication.should_publish(Some(snapshot.clone()), start + Duration::from_secs(31))
        );
        snapshot.pending_attention[0].actionability.macos = true;
        assert!(
            publication
                .should_publish(Some(snapshot.clone()), start + Duration::from_millis(31001))
        );
        snapshot.pending_attention.clear();
        assert!(publication.should_publish(Some(snapshot), start + Duration::from_millis(31002)));
        assert!(publication.should_publish(None, start + Duration::from_secs(32)));
        assert!(!publication.should_publish(None, start + Duration::from_secs(100)));
    }

    // The Windows writer blocks its thread; the pipe's accept loop needs a
    // free worker.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn given_private_launch_intake_should_reject_foreign_session_child_and_token_then_invalidate_on_exit()
     {
        let (prepared, _tmp) = fixture("claude", Some("root"));
        let path = prepared.state._artifacts.signal_address.clone();
        let states = SignalStates::default();
        let channels = Arc::new(tokio::sync::Mutex::new(HashMap::new()));
        let (status, mut updates) = tokio::sync::broadcast::channel(64);
        channels.lock().await.insert("project".into(), status);
        let (tx, rx) = tokio::sync::watch::channel(None);
        prepared.activate(&states, "project", "/owned", 123, rx, channels);
        for (token, payload) in [
            (
                "wrong",
                json!({"session_id":"root","hook_event_name":"UserPromptSubmit"}),
            ),
            (
                "private-fixture-token",
                json!({"session_id":"foreign","hook_event_name":"UserPromptSubmit"}),
            ),
            (
                "private-fixture-token",
                json!({"session_id":"root","agent_id":"child","hook_event_name":"UserPromptSubmit"}),
            ),
        ] {
            send(&path, token, payload);
        }
        settle().await;
        assert_eq!(
            states.lock().unwrap()["ag-test"]
                .snapshot()
                .unwrap()
                .observation
                .state,
            ObservationState::Unavailable
        );
        send(
            &path,
            "private-fixture-token",
            json!({"session_id":"root","hook_event_name":"UserPromptSubmit"}),
        );
        settle().await;
        let state = states.lock().unwrap()["ag-test"].clone();
        assert_eq!(state.snapshot().unwrap().activity, SignalActivity::Unknown);
        assert!(updates.try_recv().is_ok());
        send(
            &path,
            "private-fixture-token",
            json!({"session_id":"root","hook_event_name":"PermissionRequest"}),
        );
        send(
            &path,
            "private-fixture-token",
            json!({"session_id":"root","hook_event_name":"Stop"}),
        );
        settle().await;
        assert!(state.snapshot().unwrap().pending_attention.is_empty());
        assert!(state.snapshot().unwrap().outcome.is_none());
        tx.send(Some(0)).unwrap();
        settle().await;
        assert_eq!(
            state.snapshot().unwrap().observation.state,
            ObservationState::ProcessDead
        );
        assert!(state.snapshot().unwrap().outcome.is_none());
    }
    /// The shipped `pi.mjs` writes straight to the pipe (`net.connect`) on
    /// Windows; drive it with a stub `pi` host under Node.
    #[cfg(windows)]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn given_pi_extension_under_node_should_write_frames_to_the_pipe() {
        use tokio::io::AsyncWriteExt;
        let Ok(node) = which_node() else {
            eprintln!("node not found; skipping");
            return;
        };
        // The extension gives each frame 100ms and drops it after that
        // (fail-open, as the Unix helper does); a machine loaded by other
        // suites can starve one attempt, so run the stub host up to three
        // times and require one complete Working sequence.
        let mut activity = SignalActivity::Unknown;
        for _attempt in 0..3 {
            let (prepared, tmp) = fixture("pi", None);
            let address = prepared.state._artifacts.signal_address.clone();
            let extension = tmp.path().join("pi.mjs");
            std::fs::write(
                &extension,
                include_bytes!("../../assets/agent-signals/pi.mjs"),
            )
            .unwrap();
            let script = r#"
                const { pathToFileURL } = await import('node:url');
                const ext = (await import(pathToFileURL(process.argv[1]).href)).default;
                const on = {};
                ext({ on: (name, f) => { on[name] = f; } });
                const ctx = { sessionManager: { getSessionId: () => 'root' } };
                process.stdin.once('data', () => {
                  on.session_start({}, ctx);
                  setTimeout(() => on.agent_start({}, ctx), 300);
                  setTimeout(() => process.exit(0), 2000);
                });
            "#;
            let mut child = tokio::process::Command::new(&node)
                .args(["--input-type=module", "-e", script])
                .arg(&extension)
                .env("PU_SIGNAL_SOCKET", &address)
                .env("PU_SIGNAL_TOKEN", "private-fixture-token")
                .stdin(std::process::Stdio::piped())
                .kill_on_drop(true)
                .spawn()
                .unwrap();
            let states = SignalStates::default();
            let (_tx, rx) = tokio::sync::watch::channel(None);
            prepared.activate(
                &states,
                "project",
                "/owned",
                child.id().unwrap(),
                rx,
                Arc::default(),
            );
            child
                .stdin
                .as_mut()
                .unwrap()
                .write_all(b"go\n")
                .await
                .unwrap();
            assert!(child.wait().await.unwrap().success());
            // The frames are in the pipe once node exits, but the intake task
            // reduces them asynchronously: wait for it rather than racing it.
            let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
            activity = loop {
                let activity = states.lock().unwrap()["ag-test"]
                    .snapshot()
                    .unwrap()
                    .activity;
                if activity == SignalActivity::Working || tokio::time::Instant::now() >= deadline {
                    break activity;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            };
            if activity == SignalActivity::Working {
                break;
            }
        }
        assert_eq!(activity, SignalActivity::Working);
    }
    #[cfg(windows)]
    fn which_node() -> Result<PathBuf, ()> {
        std::env::split_paths(&std::env::var_os("PATH").ok_or(())?)
            .map(|dir| dir.join("node.exe"))
            .find(|p| p.is_file())
            .ok_or(())
    }
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn given_pi_registration_should_require_owned_pid_and_detect_dropped_observations() {
        let (prepared, _tmp) = fixture("pi", None);
        let path = prepared.state._artifacts.signal_address.clone();
        let states = SignalStates::default();
        let (_tx, rx) = tokio::sync::watch::channel(None);
        prepared.activate(&states, "project", "/owned", 123, rx, Arc::default());
        send(
            &path,
            "private-fixture-token",
            json!({"session_id":"foreign","native_pid":999,"sequence":1,"type":"observer_bound"}),
        );
        settle().await;
        assert!(states.lock().unwrap()["ag-test"].snapshot().is_none());
        send(
            &path,
            "private-fixture-token",
            json!({"session_id":"root","native_pid":123,"sequence":1,"type":"observer_bound"}),
        );
        send(
            &path,
            "private-fixture-token",
            json!({"session_id":"root","native_pid":123,"sequence":2,"type":"agent_start"}),
        );
        settle().await;
        assert_eq!(
            states.lock().unwrap()["ag-test"]
                .snapshot()
                .unwrap()
                .activity,
            SignalActivity::Working
        );
        send(
            &path,
            "private-fixture-token",
            json!({"session_id":"root","native_pid":123,"sequence":4,"type":"agent_settled","outcome":"completed","aborted":false}),
        );
        settle().await;
        let snapshot = states.lock().unwrap()["ag-test"].snapshot().unwrap();
        assert_eq!(snapshot.observation.state, ObservationState::Lost);
        assert!(snapshot.outcome.is_none());
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn given_installed_old_helper_should_preserve_native_launch_and_leave_no_registration() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let bin = tmp.path().join("bin");
        std::fs::create_dir(&bin).unwrap();
        for (name, body) in [
            ("pu", "#!/bin/sh\nprintf unsupported\nexit 2\n"),
            ("pi", "#!/bin/sh\nprintf '1.3.0\\n'\n"),
        ] {
            let path = bin.join(name);
            std::fs::write(&path, body).unwrap();
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        let mut args = vec!["--session-id".into(), "native-exact".into()];
        let original = args.clone();
        let mut env = vec![
            ("PATH".into(), bin.to_string_lossy().into_owned()),
            ("HOME".into(), tmp.path().to_string_lossy().into_owned()),
            ("PU_SIGNAL_TOKEN".into(), "parent-must-not-leak".into()),
        ];
        let prepared = prepare(
            tmp.path().to_str().unwrap(),
            tmp.path().to_str().unwrap(),
            "agent",
            None,
            "pi",
            Some("native-exact"),
            false,
            bin.join("pi").to_str().unwrap(),
            &mut args,
            &mut env,
        )
        .await;
        assert!(prepared.is_none());
        assert_eq!(args, original);
        assert!(!env.iter().any(|(k, _)| k.starts_with("PU_SIGNAL_")));
        assert_eq!(
            std::fs::read_dir(tmp.path().join(".pu/agent-signals"))
                .unwrap()
                .count(),
            0
        );
    }
    #[tokio::test]
    async fn given_existing_launch_settings_or_unavailable_artifact_storage_should_not_replace_configuration()
     {
        let tmp = tempfile::tempdir().unwrap();
        let mut args = vec!["--settings".into(), "user-settings.json".into()];
        let original = args.clone();
        let mut env = Vec::new();
        let prepared = prepare(
            tmp.path().to_str().unwrap(),
            tmp.path().to_str().unwrap(),
            "agent",
            None,
            "claude",
            Some("native"),
            false,
            "claude",
            &mut args,
            &mut env,
        )
        .await;
        assert!(prepared.is_some());
        drop(prepared);
        assert_eq!(args, original);
        std::fs::remove_dir_all(tmp.path().join(".pu/agent-signals")).unwrap();
        std::fs::write(tmp.path().join(".pu/agent-signals"), b"unavailable storage").unwrap();
        assert!(install::Artifacts::new(tmp.path(), "launch").is_err());
        assert_eq!(
            std::fs::read(tmp.path().join(".pu/agent-signals")).unwrap(),
            b"unavailable storage"
        );
    }
    fn intake(tool: &str) -> Intake {
        Intake {
            tool: tool.into(),
            version: "fixture".into(),
            pid: 123,
            sequence: 0,
            previous_native: None,
        }
    }
    fn receive(intake: &mut Intake, state: &LaunchState, payload: Value) {
        intake.receive(
            state,
            &serde_json::to_vec(&json!({"token":"private-fixture-token","payload":payload}))
                .unwrap(),
            "private-fixture-token",
        );
    }
    #[tokio::test]
    async fn given_rejectable_submit_after_completion_should_not_clear_confirmed_outcome() {
        let (prepared, _tmp) = fixture("claude", Some("root"));
        let identity = prepared.state.identity.lock().unwrap().clone();
        let make = |sequence, kind| SignalEvent {
            schema_version: 1,
            tool: "qualified-fixture".into(),
            tool_version: "1".into(),
            adapter_version: "1".into(),
            identity: identity.clone(),
            parent_session_id: None,
            turn_id: Some("A".into()),
            event_id: format!("seed-{sequence}"),
            sequence,
            occurred_at_ms: now_ms(),
            confidence: SignalConfidence::Authoritative,
            kind,
        };
        {
            let mut r = prepared.state.reducer.lock().unwrap();
            let r = r.as_mut().unwrap();
            r.apply(make(1, SignalEventKind::Start), now_ms());
            r.apply(
                make(
                    2,
                    SignalEventKind::Outcome {
                        kind: OutcomeKind::Completed,
                    },
                ),
                now_ms(),
            );
        }
        let before = prepared.state.snapshot().unwrap();
        let mut adapter = intake("claude");
        adapter.sequence = 2;
        for id in ["B", "B", "C"] {
            receive(
                &mut adapter,
                &prepared.state,
                json!({"session_id":"root","hook_event_name":"UserPromptSubmit","prompt_id":id}),
            );
        }
        assert_eq!(prepared.state.snapshot().unwrap(), before);
    }
    #[tokio::test]
    async fn given_native_claude_prompt_identity_should_reject_delayed_failure_and_deduplicate_raw_replay()
     {
        let (prepared, _tmp) = fixture("claude", Some("root"));
        let mut adapter = intake("claude");
        // Submission itself is weak; seed a separately confirmed current turn B.
        let identity = prepared.state.identity.lock().unwrap().clone();
        prepared
            .state
            .reducer
            .lock()
            .unwrap()
            .as_mut()
            .unwrap()
            .apply(
                SignalEvent {
                    schema_version: 1,
                    tool: "qualified-fixture".into(),
                    tool_version: "1".into(),
                    adapter_version: "1".into(),
                    identity,
                    parent_session_id: None,
                    turn_id: Some("B".into()),
                    event_id: "real-start-B".into(),
                    sequence: 1,
                    occurred_at_ms: now_ms(),
                    confidence: SignalConfidence::Authoritative,
                    kind: SignalEventKind::Start,
                },
                now_ms(),
            );
        adapter.sequence = 1;
        for prompt in [Some("A"), Some("A"), None] {
            let mut payload = json!({"session_id":"root","hook_event_name":"StopFailure"});
            if let Some(id) = prompt {
                payload["prompt_id"] = id.into();
            }
            receive(&mut adapter, &prepared.state, payload);
        }
        let snapshot = prepared.state.snapshot().unwrap();
        assert_eq!(snapshot.activity, SignalActivity::Working);
        assert!(snapshot.outcome.is_none());
        let raw = json!({"session_id":"root","hook_event_name":"StopFailure","prompt_id":"B"});
        receive(&mut adapter, &prepared.state, raw.clone());
        let first = prepared.state.snapshot().unwrap();
        assert_eq!(
            first.outcome.as_ref().unwrap().turn_id.as_deref(),
            Some("B")
        );
        receive(&mut adapter, &prepared.state, raw);
        assert_eq!(prepared.state.snapshot().unwrap(), first);
    }
    #[tokio::test]
    async fn given_pi_frames_should_validate_registration_and_every_pid_sequence_timestamp_before_state_changes()
     {
        let (prepared, _tmp) = fixture("pi", None);
        let mut adapter = intake("pi");
        for ty in ["agent_start", "unknown", "observer_bound"] {
            let mut payload = json!({"type":ty,"session_id":"root","native_pid":123,"sequence":1,"occurred_at_ms":now_ms()});
            if ty == "observer_bound" {
                payload.as_object_mut().unwrap().remove("sequence");
            }
            receive(&mut adapter, &prepared.state, payload);
            assert!(prepared.state.snapshot().is_none());
        }
        receive(
            &mut adapter,
            &prepared.state,
            json!({"type":"observer_bound","session_id":"child","agent_id":"child","native_pid":123,"sequence":1,"occurred_at_ms":now_ms()}),
        );
        assert!(prepared.state.snapshot().is_none());
        receive(
            &mut adapter,
            &prepared.state,
            json!({"type":"observer_bound","session_id":"root","native_pid":123,"sequence":1,"occurred_at_ms":now_ms()}),
        );
        assert!(prepared.state.snapshot().is_some());
        for (pid, sequence, time) in [
            (Some(999), Some(2), Some(now_ms())),
            (None, Some(2), Some(now_ms())),
            (Some(123), None, Some(now_ms())),
            (Some(123), Some(2), None),
        ] {
            let mut frame = json!({"type":"agent_start","session_id":"root"});
            if let Some(v) = pid {
                frame["native_pid"] = v.into();
            }
            if let Some(v) = sequence {
                frame["sequence"] = v.into();
            }
            if let Some(v) = time {
                frame["occurred_at_ms"] = v.into();
            }
            receive(&mut adapter, &prepared.state, frame);
            assert_eq!(
                prepared.state.snapshot().unwrap().observation.state,
                ObservationState::Unavailable
            );
            assert_eq!(adapter.previous_native, Some(1));
        }
        for payload in [
            json!({"type":"agent_start","session_id":"root","native_pid":123,"sequence":u64::MAX,"occurred_at_ms":now_ms()}),
            json!({"type":"ui_prompt_start","kind":"input","request_id":"","session_id":"root","native_pid":123,"sequence":9,"occurred_at_ms":now_ms()}),
            json!({"type":"agent_settled","outcome":"completed","session_id":"root","native_pid":123,"sequence":9,"occurred_at_ms":now_ms()}),
        ] {
            receive(&mut adapter, &prepared.state, payload);
            assert_eq!(adapter.previous_native, Some(1));
        }
        let valid = json!({"type":"agent_start","session_id":"root","native_pid":123,"sequence":2,"occurred_at_ms":now_ms()});
        receive(&mut adapter, &prepared.state, valid.clone());
        let snapshot = prepared.state.snapshot().unwrap();
        assert_eq!(snapshot.activity, SignalActivity::Working);
        receive(&mut adapter, &prepared.state, valid);
        assert_eq!(prepared.state.snapshot().unwrap(), snapshot);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn native_owner_qualification_must_enable_only_exact_adapter_and_current_requests() {
        use std::os::unix::fs::PermissionsExt;
        let (mut prepared, tmp) = fixture("claude", Some("root"));
        let now = now_ms();
        let path = tmp.path().join(".pu/agent-signals/qualifications.json");
        std::fs::write(&path,serde_json::to_vec(&json!({"schema_version":1,"qualifications":[{"tool":"claude","tool_version":"2.1.300","adapter_version":"2","platform":"macos","execution_mode":"interactive_pty","owner_confirmed":true,"proof_record_url":"https://pagespace.ai/dashboard/drive/fixture-only","qualified_at_ms":now-1000,"expires_at_ms":now+10000}]})).unwrap()).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        Arc::get_mut(&mut prepared.state)
            .unwrap()
            .macos_qualification =
            qualification::load(tmp.path().to_str().unwrap(), "claude", "2", &[]).await;
        let identity = prepared.state.identity.lock().unwrap().clone();
        let mut reducer = SignalReducer::new(identity, limits()).unwrap();
        let event = SignalEvent {
            schema_version: 1,
            tool: "claude".into(),
            // The grant was recorded on 2.1.300; later releases stay qualified.
            tool_version: "2.1.304".into(),
            adapter_version: "2".into(),
            identity: prepared.state.identity.lock().unwrap().clone(),
            parent_session_id: None,
            turn_id: None,
            event_id: "native-request".into(),
            sequence: 1,
            occurred_at_ms: now,
            confidence: SignalConfidence::Authoritative,
            kind: SignalEventKind::AttentionPending {
                request: PendingAttention {
                    request_id: "native".into(),
                    kind: AttentionKind::Question,
                    event_id: "native-request".into(),
                    turn_id: None,
                    confidence: SignalConfidence::Authoritative,
                    actionability: AttentionActionability::default(),
                },
            },
        };
        reducer.apply(event, now);
        *prepared.state.native_snapshot.lock().unwrap() = Some(reducer.snapshot(now));
        assert!(
            prepared.state.snapshot().unwrap().pending_attention[0]
                .actionability
                .macos
        );

        assert!(
            qualification::load(tmp.path().to_str().unwrap(), "claude", "1", &[])
                .await
                .is_none()
        );
        assert!(
            !prepared.state.snapshot().unwrap().pending_attention[0]
                .actionability
                .ios
        );
        {
            let mut native = prepared.state.native_snapshot.lock().unwrap();
            native
                .as_mut()
                .unwrap()
                .evidence
                .as_mut()
                .unwrap()
                .adapter_version = "1".into();
        }
        assert!(
            !prepared.state.snapshot().unwrap().pending_attention[0]
                .actionability
                .macos
        );
        {
            let mut native = prepared.state.native_snapshot.lock().unwrap();
            native
                .as_mut()
                .unwrap()
                .evidence
                .as_mut()
                .unwrap()
                .adapter_version = "2".into();
            native.as_mut().unwrap().pending_attention[0].confidence =
                SignalConfidence::SourceConfirmed;
        }
        assert!(
            !prepared.state.snapshot().unwrap().pending_attention[0]
                .actionability
                .macos
        );
        {
            let mut native = prepared.state.native_snapshot.lock().unwrap();
            native.as_mut().unwrap().pending_attention[0].confidence =
                SignalConfidence::Authoritative;
            native.as_mut().unwrap().observation.state = ObservationState::Disconnected;
        }
        assert!(
            !prepared.state.snapshot().unwrap().pending_attention[0]
                .actionability
                .macos
        );
        prepared
            .state
            .native_snapshot
            .lock()
            .unwrap()
            .as_mut()
            .unwrap()
            .observation
            .state = ObservationState::Current;
        assert!(
            !prepared
                .state
                .macos_qualification
                .as_ref()
                .unwrap()
                .current(now + 20000)
        );
        let final_snapshot = prepared.state.snapshot().unwrap();
        assert!(!final_snapshot.pending_attention[0].actionability.macos);
        assert_eq!(final_snapshot.pending_attention[0].request_id, "native");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn given_owner_qualification_should_mark_only_authoritative_pending_and_withdraw_after_expiry()
     {
        use std::os::unix::fs::PermissionsExt;
        let (mut prepared, tmp) = fixture("pi", None);
        let now = now_ms();
        let path = tmp.path().join(".pu/agent-signals/qualifications.json");
        std::fs::write(&path,serde_json::to_vec(&json!({"schema_version":1,"qualifications":[{"tool":"pi","tool_version":"1.3.0","adapter_version":"1","platform":"macos","execution_mode":"interactive_pty","owner_confirmed":true,"proof_record_url":"https://pagespace.ai/dashboard/drive/fixture-only","qualified_at_ms":now-1000,"expires_at_ms":now+1000}]})).unwrap()).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        let grant = qualification::load(tmp.path().to_str().unwrap(), "pi", "1", &[])
            .await
            .unwrap();
        Arc::get_mut(&mut prepared.state)
            .unwrap()
            .macos_qualification = Some(grant);
        let mut adapter = intake("pi");
        adapter.version = "1.3.0".into();
        receive(
            &mut adapter,
            &prepared.state,
            json!({"type":"observer_bound","session_id":"root","native_pid":123,"sequence":1,"occurred_at_ms":now_ms()}),
        );
        receive(
            &mut adapter,
            &prepared.state,
            json!({"type":"ui_prompt_start","kind":"confirm","request_id":"ui-1","session_id":"root","native_pid":123,"sequence":2,"occurred_at_ms":now_ms()}),
        );
        let request = prepared
            .state
            .snapshot()
            .unwrap()
            .pending_attention
            .remove(0);
        assert!(request.actionability.macos);
        assert!(!request.actionability.ios);
        assert_eq!(request.kind, AttentionKind::Question);
        tokio::time::sleep(Duration::from_millis(1020)).await;
        assert!(
            !prepared.state.snapshot().unwrap().pending_attention[0]
                .actionability
                .macos
        );
        assert!(
            qualification::load(
                tmp.path().to_str().unwrap(),
                "pi",
                "1",
                &["--mode".into(), "rpc".into()]
            )
            .await
            .is_none()
        );
    }
    #[tokio::test]
    async fn given_reducer_invalid_pi_envelope_should_not_burn_a_corrected_source_sequence() {
        let (prepared, _tmp) = fixture("pi", None);
        let mut adapter = intake("pi");
        receive(
            &mut adapter,
            &prepared.state,
            json!({"type":"observer_bound","session_id":"root","native_pid":123,"sequence":1,"occurred_at_ms":now_ms()}),
        );
        for malformed in [json!(""), json!("invalid/turn"), json!(42)] {
            receive(
                &mut adapter,
                &prepared.state,
                json!({"type":"agent_start","session_id":"root","native_pid":123,"sequence":2,"turn_id":malformed,"occurred_at_ms":now_ms()}),
            );
            assert_eq!(adapter.previous_native, Some(1));
            assert_eq!(
                prepared.state.snapshot().unwrap().observation.state,
                ObservationState::Unavailable
            );
        }
        receive(
            &mut adapter,
            &prepared.state,
            json!({"type":"agent_start","session_id":"root","native_pid":123,"sequence":2,"occurred_at_ms":now_ms()}),
        );
        assert_eq!(
            prepared.state.snapshot().unwrap().activity,
            SignalActivity::Working
        );
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn given_projection_clock_rollback_should_latch_denied_without_resolving_native_attention()
     {
        use std::os::unix::fs::PermissionsExt;
        let (mut prepared, tmp) = fixture("pi", None);
        let now = now_ms();
        let path = tmp.path().join(".pu/agent-signals/qualifications.json");
        std::fs::write(&path,serde_json::to_vec(&json!({"schema_version":1,"qualifications":[{"tool":"pi","tool_version":"1.3.0","adapter_version":"1","platform":"macos","execution_mode":"interactive_pty","owner_confirmed":true,"proof_record_url":"https://pagespace.ai/dashboard/drive/fixture-only","qualified_at_ms":now-1000,"expires_at_ms":now+60000}]})).unwrap()).unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).unwrap();
        Arc::get_mut(&mut prepared.state)
            .unwrap()
            .macos_qualification =
            qualification::load(tmp.path().to_str().unwrap(), "pi", "1", &[]).await;
        let mut adapter = intake("pi");
        adapter.version = "1.3.0".into();
        receive(
            &mut adapter,
            &prepared.state,
            json!({"type":"observer_bound","session_id":"root","native_pid":123,"sequence":1,"occurred_at_ms":now_ms()}),
        );
        receive(
            &mut adapter,
            &prepared.state,
            json!({"type":"ui_prompt_start","kind":"input","request_id":"ui-1","session_id":"root","native_pid":123,"sequence":2,"occurred_at_ms":now_ms()}),
        );
        assert!(
            prepared.state.snapshot().unwrap().pending_attention[0]
                .actionability
                .macos
        );
        let grant = prepared.state.macos_qualification.as_ref().unwrap();
        assert!(grant.current(now + 30000));
        let snapshot = prepared.state.snapshot().unwrap();
        assert_eq!(snapshot.pending_attention.len(), 1);
        assert_eq!(snapshot.pending_attention[0].request_id, "ui-1");
        assert!(!snapshot.pending_attention[0].actionability.macos);
        assert!(!snapshot.pending_attention[0].actionability.ios);
        assert!(!grant.current(now + 40000));
        assert!(snapshot.outcome.is_none());
    }
    #[test]
    fn given_private_token_should_compare_entire_equal_length_input() {
        assert!(token_matches(b"private", b"private"));
        assert!(!token_matches(b"private", b"privatx"));
        assert!(!token_matches(b"private", b"private-longer"));
    }
}
