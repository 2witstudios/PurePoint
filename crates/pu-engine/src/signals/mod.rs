//! Observation-only, launch-scoped native adapters. No process or input ownership.
mod install;
mod normalize;
mod qualification;

use pu_core::signals::*;
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::net::UnixDatagram;

pub(crate) type SignalStates = Arc<Mutex<HashMap<String, Arc<LaunchState>>>>;

pub(crate) struct LaunchState {
    reducer: Mutex<Option<SignalReducer>>,
    identity: Mutex<SignalIdentity>,
    macos_qualification: Option<qualification::Grant>,
    // Dropping the last state removes only exact owned artifacts.
    _artifacts: install::Artifacts,
}

impl LaunchState {
    pub(crate) fn snapshot(&self) -> Option<SignalSnapshot> {
        let mut snapshot = self.reducer.lock().ok()?.as_ref()?.snapshot(now_ms());
        if !self
            .macos_qualification
            .as_ref()
            .is_some_and(|grant| grant.current_now())
        {
            for request in &mut snapshot.pending_attention {
                request.actionability.macos = false;
            }
        }
        Some(snapshot)
    }
}

pub(crate) struct Prepared {
    socket: UnixDatagram,
    token: String,
    tool: String,
    version: String,
    state: Arc<LaunchState>,
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
    let sibling = std::env::current_exe().ok()?.with_file_name("pu");
    if sibling.is_file() {
        return Some(sibling);
    }
    let path = env.iter().rev().find(|(k, _)| k == "PATH")?.1.clone();
    path.split(':')
        .map(|p| Path::new(p).join("pu"))
        .find(|p| p.is_absolute() && p.is_file())
}

/// Add launch-only observation artifacts. Failure never prevents the native launch.
/// OpenCode/Gemini/Codex/Cursor stay uninstalled until their existing ownership and
/// exact session binding paths are qualified; their normalizers remain fixture-tested.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn prepare(
    project: &str,
    agent: &str,
    worktree: Option<String>,
    tool: &str,
    session: Option<&str>,
    command: &str,
    args: &mut Vec<String>,
    env: &mut Vec<(String, String)>,
) -> Option<Prepared> {
    env.retain(|(key, _)| !key.starts_with("PU_SIGNAL_"));
    if !matches!(tool, "claude" | "pi") || Path::new(command).file_name()?.to_str()? != tool {
        return None;
    }
    if tool == "claude"
        && (session.is_none()
            || args
                .iter()
                .any(|arg| arg == "--settings" || arg.starts_with("--settings=")))
    {
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
    let helper_probe = tokio::process::Command::new(&helper)
        .arg("__agent-signal")
        .env_clear()
        .env("HOME", &home)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .status();
    if !tokio::time::timeout(Duration::from_millis(500), helper_probe)
        .await
        .ok()?
        .ok()?
        .success()
    {
        return None;
    }
    let version = if tool == "claude" { "2.1.300" } else { "1.3.0" };
    let probe = async {
        use tokio::io::AsyncReadExt;
        let mut child = tokio::process::Command::new(command)
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
        // Exact release token, not a prefix accepting an incompatible later build.
        String::from_utf8_lossy(&bytes)
            .split_whitespace()
            .any(|v| v == version)
            .then_some(())
    };
    let qualified = tokio::time::timeout(Duration::from_millis(500), probe).await;
    let _ = std::fs::remove_dir(&home);
    qualified.ok()??;
    let socket = UnixDatagram::bind(artifacts.socket_directory.join("s")).ok()?;
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
    let macos_qualification = qualification::load(project, tool, version, args).await;
    let config = if tool == "claude" {
        let p = artifacts
            .write("claude-settings.json", install::claude_settings(&helper))
            .ok()?;
        vec!["--settings".into(), p.to_string_lossy().into_owned()]
    } else {
        let p = artifacts
            .write(
                "pi.mjs",
                include_bytes!("../../assets/agent-signals/pi.mjs").to_vec(),
            )
            .ok()?;
        vec!["--extension".into(), p.to_string_lossy().into_owned()]
    };
    args.extend(config);
    env.extend([
        ("PU_SIGNAL_TOKEN".into(), token.clone()),
        (
            "PU_SIGNAL_SOCKET".into(),
            artifacts
                .socket_directory
                .join("s")
                .to_string_lossy()
                .into_owned(),
        ),
        (
            "PU_SIGNAL_HELPER".into(),
            helper.to_string_lossy().into_owned(),
        ),
    ]);
    Some(Prepared {
        socket,
        token,
        tool: tool.into(),
        version: version.into(),
        state: Arc::new(LaunchState {
            reducer: Mutex::new(reducer),
            identity: Mutex::new(identity),
            macos_qualification,
            _artifacts: artifacts,
        }),
    })
}

impl Prepared {
    pub(crate) fn activate(
        self,
        states: &SignalStates,
        project: &str,
        pid: u32,
        mut exit: tokio::sync::watch::Receiver<Option<i32>>,
        channels: Arc<tokio::sync::Mutex<HashMap<String, tokio::sync::broadcast::Sender<()>>>>,
    ) {
        let id = self.state.identity.lock().unwrap().agent_id.clone();
        states.lock().unwrap().insert(id, self.state.clone());
        let weak = Arc::downgrade(&self.state);
        let project = project.to_string();
        let Self {
            socket,
            token,
            tool,
            version,
            state: _,
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn fixture(tool: &str, session: Option<&str>) -> (Prepared, tempfile::TempDir) {
        let tmp = tempfile::tempdir().unwrap();
        let launch = pu_core::id::session_id();
        let artifacts = install::Artifacts::new(tmp.path(), &launch).unwrap();
        let socket = UnixDatagram::bind(artifacts.socket_directory.join("s")).unwrap();
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
                    macos_qualification: None,
                    _artifacts: artifacts,
                }),
            },
            tmp,
        )
    }
    async fn settle() {
        tokio::time::sleep(Duration::from_millis(30)).await;
    }
    fn send(
        socket: &std::os::unix::net::UnixDatagram,
        path: &Path,
        token: &str,
        mut payload: Value,
    ) {
        payload
            .as_object_mut()
            .unwrap()
            .entry("occurred_at_ms")
            .or_insert_with(|| now_ms().into());
        socket
            .send_to(
                &serde_json::to_vec(&json!({"token":token,"payload":payload})).unwrap(),
                path,
            )
            .unwrap();
    }
    #[tokio::test]
    async fn given_private_launch_intake_should_reject_foreign_session_child_and_token_then_invalidate_on_exit()
     {
        let (prepared, _tmp) = fixture("claude", Some("root"));
        let path = prepared.state._artifacts.socket_directory.join("s");
        let states = SignalStates::default();
        let channels = Arc::new(tokio::sync::Mutex::new(HashMap::new()));
        let (status, mut updates) = tokio::sync::broadcast::channel(64);
        channels.lock().await.insert("project".into(), status);
        let (tx, rx) = tokio::sync::watch::channel(None);
        prepared.activate(&states, "project", 123, rx, channels);
        let socket = std::os::unix::net::UnixDatagram::unbound().unwrap();
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
            send(&socket, &path, token, payload);
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
            &socket,
            &path,
            "private-fixture-token",
            json!({"session_id":"root","hook_event_name":"UserPromptSubmit"}),
        );
        settle().await;
        let state = states.lock().unwrap()["ag-test"].clone();
        assert_eq!(state.snapshot().unwrap().activity, SignalActivity::Unknown);
        assert!(updates.try_recv().is_ok());
        send(
            &socket,
            &path,
            "private-fixture-token",
            json!({"session_id":"root","hook_event_name":"PermissionRequest"}),
        );
        send(
            &socket,
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
    #[tokio::test]
    async fn given_pi_registration_should_require_owned_pid_and_detect_dropped_observations() {
        let (prepared, _tmp) = fixture("pi", None);
        let path = prepared.state._artifacts.socket_directory.join("s");
        let states = SignalStates::default();
        let (_tx, rx) = tokio::sync::watch::channel(None);
        prepared.activate(&states, "project", 123, rx, Arc::default());
        let socket = std::os::unix::net::UnixDatagram::unbound().unwrap();
        send(
            &socket,
            &path,
            "private-fixture-token",
            json!({"session_id":"foreign","native_pid":999,"sequence":1,"type":"observer_bound"}),
        );
        settle().await;
        assert!(states.lock().unwrap()["ag-test"].snapshot().is_none());
        send(
            &socket,
            &path,
            "private-fixture-token",
            json!({"session_id":"root","native_pid":123,"sequence":1,"type":"observer_bound"}),
        );
        send(
            &socket,
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
            &socket,
            &path,
            "private-fixture-token",
            json!({"session_id":"root","native_pid":123,"sequence":4,"type":"agent_settled","outcome":"completed","aborted":false}),
        );
        settle().await;
        let snapshot = states.lock().unwrap()["ag-test"].snapshot().unwrap();
        assert_eq!(snapshot.observation.state, ObservationState::Lost);
        assert!(snapshot.outcome.is_none());
    }
    #[tokio::test]
    async fn given_installed_old_helper_should_preserve_native_launch_and_leave_no_registration() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let bin = tmp.path().join("bin");
        std::fs::create_dir(&bin).unwrap();
        for (name, body) in [
            ("pu", "#!/bin/sh\nprintf unsupported\nexit 2\n"),
            ("claude", "#!/bin/sh\nprintf '2.1.300\\n'\n"),
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
            "agent",
            None,
            "claude",
            Some("native-exact"),
            bin.join("claude").to_str().unwrap(),
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
        assert!(
            prepare(
                tmp.path().to_str().unwrap(),
                "agent",
                None,
                "claude",
                Some("native"),
                "claude",
                &mut args,
                &mut env
            )
            .await
            .is_none()
        );
        assert_eq!(args, original);
        assert!(!tmp.path().join(".pu").exists());
        std::fs::create_dir(tmp.path().join(".pu")).unwrap();
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
    #[tokio::test]
    async fn given_owner_qualification_should_mark_only_authoritative_pending_and_withdraw_after_expiry()
     {
        use std::os::unix::fs::PermissionsExt;
        let (mut prepared, tmp) = fixture("pi", None);
        let now = now_ms();
        let path = tmp.path().join(".pu/agent-signals/qualifications.json");
        std::fs::write(&path,serde_json::to_vec(&json!({"schema_version":1,"qualifications":[{"tool":"pi","tool_version":"1.3.0","adapter_version":"1","platform":"macos","execution_mode":"interactive_pty","owner_confirmed":true,"proof_record_url":"https://pagespace.ai/dashboard/drive/fixture-only","qualified_at_ms":now-1000,"expires_at_ms":now+1000}]})).unwrap()).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        let grant = qualification::load(tmp.path().to_str().unwrap(), "pi", "1.3.0", &[])
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
                "1.3.0",
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
            qualification::load(tmp.path().to_str().unwrap(), "pi", "1.3.0", &[]).await;
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
