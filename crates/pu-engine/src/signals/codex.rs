//! Native Codex OSC 7501 state, joined to exact owned root rollout identities.
use pu_core::signals::*;
use serde_json::Value;
use std::collections::{BTreeMap, HashSet};

use super::{LaunchState, NativeObserver, Prepared, install, now_ms};
use crate::output_buffer::OutputBuffer;
use crate::read_source::Binding;
use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::Path;
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

pub(super) struct Observer {
    expected: Option<String>,
}
pub(super) fn prepare(
    project: &str,
    agent: &str,
    worktree: Option<String>,
    session: Option<&str>,
) -> Option<Prepared> {
    if session.is_some_and(|id| !crate::read_source::valid_session_id(id)) {
        return None;
    }
    let launch = pu_core::id::session_id();
    let artifacts = install::Artifacts::new(Path::new(project), &launch).ok()?;
    let socket = tokio::net::UnixDatagram::bind(artifacts.socket_directory.join("s")).ok()?;
    Some(Prepared {
        socket,
        token: String::new(),
        tool: "codex".into(),
        version: "0.163.0".into(),
        output: None,
        state: Arc::new(LaunchState {
            reducer: Mutex::new(None),
            identity: Mutex::new(SignalIdentity {
                agent_id: agent.into(),
                worktree_id: worktree,
                launch_id: launch,
                generation: 1,
                native_session_id: session.unwrap_or("").into(),
            }),
            native_snapshot: Mutex::new(None),
            macos_qualification: None,
            _artifacts: artifacts,
        }),
        observer: Some(NativeObserver::Codex(Observer {
            expected: session.map(str::to_owned),
        })),
    })
}

#[derive(Default)]
struct Cursor {
    offset: u64,
    inode: Option<(u64, u64)>,
    partial: Vec<u8>,
    poisoned: bool,
}
impl Cursor {
    fn read(&mut self, path: &Path, history: &mut History, cwd: &str) -> Option<()> {
        if self.poisoned {
            return None;
        }
        let mut file = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(path)
            .ok()?;
        self.poisoned = true;
        let meta = file.metadata().ok()?;
        let inode = (meta.dev(), meta.ino());
        if !meta.is_file()
            || meta.uid() != unsafe { libc::geteuid() }
            || meta.len() < self.offset
            || self.inode.is_some_and(|old| old != inode)
        {
            return None;
        }
        self.inode = Some(inode);
        file.seek(SeekFrom::Start(self.offset)).ok()?;
        let mut bytes = Vec::new();
        file.take(8 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .ok()?;
        if bytes.len() > 8 * 1024 * 1024 {
            return None;
        }
        self.offset += bytes.len() as u64;
        self.partial.extend(bytes);
        let end = self
            .partial
            .iter()
            .rposition(|b| *b == b'\n')
            .map(|p| p + 1)
            .unwrap_or(0);
        let complete: Vec<_> = self.partial.drain(..end).collect();
        for line in complete.split(|b| *b == b'\n').filter(|l| !l.is_empty()) {
            if line.len() > 1024 * 1024 {
                return None;
            }
            history.record(&serde_json::from_slice::<Value>(line).ok()?, cwd)?;
        }
        if self.partial.len() > 1024 * 1024 {
            return None;
        }
        self.poisoned = false;
        Some(())
    }
    fn unchanged(&self, path: &Path) -> bool {
        self.partial.is_empty()
            && std::fs::symlink_metadata(path).is_ok_and(|m| {
                m.is_file() && Some((m.dev(), m.ino())) == self.inode && m.len() == self.offset
            })
    }
}

impl Observer {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn activate(
        self,
        state: Weak<LaunchState>,
        project: String,
        cwd: String,
        pid: u32,
        mut exit: tokio::sync::watch::Receiver<Option<i32>>,
        channels: Arc<tokio::sync::Mutex<HashMap<String, tokio::sync::broadcast::Sender<()>>>>,
        output: Arc<OutputBuffer>,
    ) {
        tokio::spawn(async move {
            let mut protocol = Protocol::default();
            let mut history = History::default();
            let mut cursor = Cursor::default();
            let mut offset = 0;
            let mut lost = false;
            let mut binding: Option<Binding> = None;
            let mut tick = tokio::time::interval(Duration::from_millis(250));
            tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                tokio::select! {_=tick.tick()=>{},_=exit.changed()=>{}}
                let Some(state) = state.upgrade() else { break };
                let dead = exit.borrow().is_some() || exit.has_changed().is_err();
                let (bytes, next, gap) = output.read_from(offset);
                offset = next;
                if gap {
                    lost = true;
                }
                protocol.feed(&bytes);
                let result = if dead || lost {
                    None
                } else {
                    if binding.is_none() {
                        binding = tokio::time::timeout(
                            Duration::from_secs(2),
                            crate::read_source::discover_codex(pid, &cwd),
                        )
                        .await
                        .ok()
                        .and_then(Result::ok);
                        if binding.as_ref().is_some_and(|b| {
                            self.expected.as_ref().is_some_and(|id| id != &b.session_id)
                        }) {
                            lost = true;
                        }
                    }
                    if let Some(binding) = binding.as_ref().filter(|_| !lost) {
                        let path = binding.path.clone();
                        let dir = cwd.clone();
                        let old = std::mem::take(&mut cursor);
                        let old_history = std::mem::take(&mut history);
                        let read = tokio::task::spawn_blocking(move || {
                            let mut c = old;
                            let mut h = old_history;
                            let valid = c.read(&path, &mut h, &dir).is_some() && c.unchanged(&path);
                            (c, h, valid)
                        })
                        .await;
                        match read {
                            Ok((c, h, valid)) => {
                                cursor = c;
                                history = h;
                                if cursor.poisoned {
                                    lost = true;
                                }
                                if valid && history.session.as_deref() == Some(&binding.session_id)
                                {
                                    let mut identity = state.identity.lock().unwrap();
                                    identity.native_session_id = binding.session_id.clone();
                                    snapshot(&identity, &protocol, &history, now_ms())
                                } else {
                                    None
                                }
                            }
                            Err(_) => {
                                lost = true;
                                None
                            }
                        }
                    } else {
                        None
                    }
                };
                let dead = dead || exit.borrow().is_some() || exit.has_changed().is_err();
                {
                    let mut value = state.native_snapshot.lock().unwrap();
                    if !dead
                        && !lost
                        && let Some(snapshot) = result
                    {
                        *value = Some(snapshot);
                    } else if let Some(snapshot) = value.as_mut() {
                        snapshot.observation.state = if dead {
                            ObservationState::ProcessDead
                        } else if lost {
                            ObservationState::Lost
                        } else {
                            ObservationState::Disconnected
                        };
                    }
                }
                if let Some(tx) = channels.lock().await.get(&project) {
                    let _ = tx.send(());
                }
                if dead {
                    break;
                }
            }
        });
    }
}

fn snapshot(
    identity: &SignalIdentity,
    protocol: &Protocol,
    history: &History,
    now: i64,
) -> Option<SignalSnapshot> {
    let status = protocol.status?;
    if status == Status::Clear {
        return None;
    }
    let mut pending_attention = Vec::new();
    if status == Status::Blocked {
        if history.tools.len() != 1 {
            return None;
        }
        let (id, kind) = history.tools.iter().next()?;
        let kind = (*kind)?;
        if protocol.kind.is_some_and(|native| native != kind) {
            return None;
        }
        pending_attention.push(PendingAttention {
            request_id: id.clone(),
            kind,
            event_id: format!("codex-request:{id}"),
            turn_id: history.turn.clone(),
            confidence: SignalConfidence::Authoritative,
            actionability: AttentionActionability::default(),
        });
    }
    Some(SignalSnapshot {
        schema_version: 1,
        identity: identity.clone(),
        activity: if status == Status::Idle {
            SignalActivity::Idle
        } else {
            SignalActivity::Working
        },
        pending_attention,
        outcome: if status == Status::Idle {
            history.outcome.clone()
        } else {
            None
        },
        observation: SignalObservation {
            state: ObservationState::Current,
            last_received_at_ms: Some(now),
            expires_at_ms: Some(now + 30_000),
        },
        evidence: Some(SignalEvidence {
            tool: "codex".into(),
            tool_version: "0.163.0".into(),
            adapter_version: "2".into(),
            confidence: SignalConfidence::Authoritative,
            event_id: format!("codex-pty:{}:{}", identity.launch_id, protocol.event_offset),
        }),
    })
}

#[derive(Default)]
struct History {
    session: Option<String>,
    turn: Option<String>,
    tools: BTreeMap<String, Option<AttentionKind>>,
    closed_turns: HashSet<String>,
    outcome: Option<SignalOutcome>,
}
impl History {
    fn retire(&mut self, turn: &str) -> Option<()> {
        if self.closed_turns.contains(turn) {
            return Some(());
        }
        if self.closed_turns.len() >= 512 {
            return None;
        }
        self.closed_turns.insert(turn.into());
        Some(())
    }
    fn record(&mut self, v: &Value, cwd: &str) -> Option<()> {
        let p = &v["payload"];
        if v["type"] == "session_meta" {
            let id = p["id"]
                .as_str()
                .filter(|s| crate::read_source::valid_session_id(s))?;
            if p["cwd"].as_str() != Some(cwd)
                || p["source"] != "cli"
                || p["cli_version"] != "0.163.0"
                || self.session.as_ref().is_some_and(|old| old != id)
            {
                return None;
            }
            self.session = Some(id.into());
            return Some(());
        }
        self.session.as_ref()?;
        match (v["type"].as_str(), p["type"].as_str()) {
            (Some("event_msg"), Some("task_started" | "turn_started")) => {
                let turn = p["turn_id"]
                    .as_str()
                    .filter(|s| crate::read_source::valid_session_id(s))?;
                if p["root_turn_id"].as_str().is_some_and(|root| root != turn) {
                    return None;
                }
                if self.turn.as_deref() == Some(turn) || self.closed_turns.contains(turn) {
                    return Some(());
                }
                if let Some(previous) = self.turn.clone() {
                    self.retire(&previous)?;
                }
                self.turn = Some(turn.into());
                self.tools.clear();
                self.outcome = None;
            }
            (Some("event_msg"), Some("task_complete" | "turn_complete" | "turn_aborted")) => {
                let turn = p["turn_id"].as_str()?;
                if self.turn.as_deref() != Some(turn) {
                    return Some(());
                }
                if p["root_turn_id"].as_str().is_some_and(|root| root != turn) {
                    return None;
                }
                if self.closed_turns.contains(turn) {
                    return Some(());
                }
                self.retire(turn)?;
                let kind = if !p["error"].is_null() {
                    OutcomeKind::Errored
                } else if p["type"] == "turn_aborted" {
                    OutcomeKind::Interrupted
                } else {
                    OutcomeKind::Completed
                };
                self.outcome = Some(SignalOutcome {
                    kind,
                    event_id: format!("codex-turn:{turn}:{}", p["type"].as_str()?),
                    turn_id: Some(turn.into()),
                    confidence: SignalConfidence::Authoritative,
                });
                self.tools.clear();
            }
            (Some("response_item"), Some("function_call" | "custom_tool_call")) => {
                let kind = match p["name"].as_str() {
                    Some("request_user_input") => Some(AttentionKind::Question),
                    Some(
                        "exec_command"
                        | "shell_command"
                        | "shell"
                        | "apply_patch"
                        | "request_permissions",
                    ) => Some(AttentionKind::Permission),
                    _ => None,
                };
                {
                    let id = p["call_id"]
                        .as_str()
                        .filter(|s| crate::read_source::valid_session_id(s))?;
                    if self.tools.len() >= 64 {
                        return None;
                    }
                    self.tools.insert(id.into(), kind);
                }
            }
            (Some("response_item"), Some("function_call_output" | "custom_tool_call_output")) => {
                self.tools.remove(p["call_id"].as_str()?);
            }
            _ => {}
        }
        Some(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Status {
    Idle,
    Working,
    Blocked,
    Clear,
}
#[derive(Default)]
struct Protocol {
    status: Option<Status>,
    kind: Option<AttentionKind>,
    offset: u64,
    event_offset: u64,
    mode: u8,
    packet: Vec<u8>,
}
impl Protocol {
    fn feed(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.offset += 1;
            match self.mode {
                0 if b == 0x1b => self.mode = 1,
                1 => match b {
                    b']' => {
                        self.mode = 2;
                        self.packet.clear();
                    }
                    b'P' | b'X' | b'^' | b'_' => self.mode = 4,
                    0x1b => {}
                    _ => self.mode = 0,
                },
                2 => match b {
                    0x1b => self.mode = 3,
                    7 => self.finish(),
                    _ if self.packet.len() < 256 => self.packet.push(b),
                    _ => self.mode = 6,
                },
                3 => {
                    if b == b'\\' {
                        self.finish();
                    } else {
                        self.mode = 6;
                    }
                }
                4 => {
                    if b == 0x1b {
                        self.mode = 5;
                    }
                }
                5 => self.mode = if b == b'\\' { 0 } else { 4 },
                6 => match b {
                    7 => self.mode = 0,
                    0x1b => self.mode = 7,
                    _ => {}
                },
                7 => self.mode = if b == b'\\' { 0 } else { 6 },
                _ => {}
            }
        }
    }
    fn finish(&mut self) {
        self.mode = 0;
        let Ok(text) = std::str::from_utf8(&self.packet) else {
            return;
        };
        let Some(body) = text.strip_prefix("7501;") else {
            return;
        };
        if body == "state=clear" {
            self.status = Some(Status::Clear);
            self.kind = None;
            self.event_offset = self.offset;
            return;
        }
        let fields: Vec<_> = body.split(':').collect();
        if !fields.contains(&"app=codex") {
            return;
        }
        self.kind = None;
        self.status = match fields.as_slice() {
            ["state=idle", "app=codex"] => Some(Status::Idle),
            ["state=working", "app=codex"] => Some(Status::Working),
            ["state=blocked", "app=codex"] => Some(Status::Blocked),
            ["state=blocked", "app=codex", kind] => {
                self.kind = match *kind {
                    "kind=permission" => Some(AttentionKind::Permission),
                    "kind=question" => Some(AttentionKind::Question),
                    _ => None,
                };
                self.kind.map(|_| Status::Blocked)
            }
            _ => None,
        };
        self.event_offset = self.offset;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn meta() -> Value {
        json!({"type":"session_meta","payload":{"id":"native","cwd":"/owned","source":"cli","cli_version":"0.163.0"}})
    }
    fn event(kind: &str, turn: &str) -> Value {
        json!({"type":"event_msg","payload":{"type":kind,"turn_id":turn,"root_turn_id":turn}})
    }
    #[test]
    fn real_native_osc_packets_survive_chunk_splits_and_require_codex_app() {
        let mut p = Protocol::default();
        for b in b"\x1b]7501;state=working:app=codex\x1b\\" {
            p.feed(&[*b]);
        }
        assert_eq!(p.status, Some(Status::Working));
        p.feed(b"\x1b]7501;state=blocked:app=codex\x1b\\");
        assert_eq!(p.status, Some(Status::Blocked));
        p.feed(b"\x1b]7501;state=idle:app=other\x1b\\");
        assert_eq!(p.status, Some(Status::Blocked));
        p.feed(b"\x1b]7501;state=idle:app=codex\x1b\\");
        assert_eq!(p.status, Some(Status::Idle));
    }
    #[test]
    fn native_turns_requests_results_and_terminal_errors_keep_exact_ids() {
        let mut h = History::default();
        h.record(&meta(), "/owned").unwrap();
        h.record(&event("task_started", "turn"), "/owned").unwrap();
        for (name, kind) in [
            ("request_user_input", AttentionKind::Question),
            ("exec_command", AttentionKind::Permission),
        ] {
            h.record(&json!({"type":"response_item","payload":{"type":"function_call","call_id":"call","name":name,"arguments":"{}"}}),"/owned").unwrap();
            assert_eq!(h.tools.get("call"), Some(&Some(kind)));
            h.record(&json!({"type":"response_item","payload":{"type":"function_call_output","call_id":"call","output":"not retained"}}),"/owned").unwrap();
            assert!(h.tools.is_empty());
        }
        let mut failed = event("task_complete", "turn");
        failed["payload"]["error"] = json!({"message":"native failure"});
        h.record(&failed, "/owned").unwrap();
        assert_eq!(h.outcome.as_ref().unwrap().kind, OutcomeKind::Errored);
        h.record(&event("task_started", "next"), "/owned").unwrap();
        assert!(h.outcome.is_none());
        h.record(&event("task_complete", "turn"), "/owned").unwrap();
        assert!(h.outcome.is_none());
        h.record(&event("task_complete", "next"), "/owned").unwrap();
        assert_eq!(h.outcome.as_ref().unwrap().kind, OutcomeKind::Completed);
    }
    #[test]
    fn wrong_root_or_version_never_binds() {
        for (key, value) in [
            ("source", json!("subagent")),
            ("cwd", json!("/foreign")),
            ("cli_version", json!("9.0.0")),
        ] {
            let mut h = History::default();
            let mut v = meta();
            v["payload"][key] = value;
            assert!(h.record(&v, "/owned").is_none());
            assert!(h.session.is_none());
        }
    }
    #[test]
    fn blocked_snapshot_requires_exact_single_native_call_and_idle_outcome() {
        let mut h = History::default();
        h.record(&meta(), "/owned").unwrap();
        h.record(&event("task_started", "turn"), "/owned").unwrap();
        let identity = SignalIdentity {
            agent_id: "agent".into(),
            worktree_id: None,
            launch_id: "launch".into(),
            generation: 1,
            native_session_id: "native".into(),
        };
        let mut p = Protocol::default();
        p.feed(b"\x1b]7501;state=working:app=codex\x1b\\");
        assert_eq!(
            snapshot(&identity, &p, &h, 100).unwrap().activity,
            SignalActivity::Working
        );
        p.feed(b"\x1b]7501;state=blocked:app=codex\x1b\\");
        assert!(snapshot(&identity, &p, &h, 100).is_none());
        h.tools
            .insert("question".into(), Some(AttentionKind::Question));
        assert_eq!(
            snapshot(&identity, &p, &h, 100).unwrap().pending_attention[0].request_id,
            "question"
        );
        h.tools.insert("unknown".into(), None);
        assert!(snapshot(&identity, &p, &h, 100).is_none());
        h.tools.clear();
        h.record(&event("task_complete", "turn"), "/owned").unwrap();
        p.feed(b"\x1b]7501;state=idle:app=codex\x1b\\");
        assert_eq!(
            snapshot(&identity, &p, &h, 100)
                .unwrap()
                .outcome
                .unwrap()
                .kind,
            OutcomeKind::Completed
        );
    }

    #[test]
    fn string_payloads_foreign_packets_and_oversized_sequences_cannot_forge_state() {
        let mut p = Protocol::default();
        p.feed(b"\x1b]7501;state=working:app=codex\x1b\\");
        p.feed(b"\x1bP\x1b]7501;state=idle:app=codex\x1b\\");
        assert_eq!(p.status, Some(Status::Working));
        p.feed(b"\x1b]7501;state=idle:app=other\x1b\\");
        assert_eq!(p.status, Some(Status::Working));
        p.feed(b"\x1b]");
        p.feed(&vec![b'a'; 4096]);
        p.feed(b"\x1b\\");
        assert!(p.packet.len() <= 256);
        p.feed(b"\x1b]7501;state=clear\x1b\\");
        assert_eq!(p.status, Some(Status::Clear));
    }

    #[test]
    fn superseded_and_closed_turn_replays_cannot_erase_native_progress() {
        let mut h = History::default();
        h.record(&meta(), "/owned").unwrap();
        h.record(&event("task_started", "A"), "/owned").unwrap();
        h.record(&event("task_started", "B"), "/owned").unwrap();
        h.record(&event("task_started", "A"), "/owned").unwrap();
        h.record(&event("task_complete", "B"), "/owned").unwrap();
        assert_eq!(h.outcome.as_ref().unwrap().turn_id.as_deref(), Some("B"));
        h.record(&event("task_started", "B"), "/owned").unwrap();
        assert_eq!(h.outcome.as_ref().unwrap().kind, OutcomeKind::Completed);
    }

    #[tokio::test]
    #[ignore = "requires sandbox native Codex probe context"]
    async fn native_interactive_observer_probe() {
        let path =
            std::env::var("PUREPOINT_CODEX_PROBE_CONTEXT").expect("sandbox context required");
        let context: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        let cwd = context["cwd"].as_str().unwrap();
        let output = Arc::new(OutputBuffer::new());
        let prepared = prepare(cwd, "probe", None, None)
            .unwrap()
            .with_output(output.clone());
        let states = Arc::new(Mutex::new(HashMap::new()));
        let (tx, rx) = tokio::sync::watch::channel(None);
        prepared.activate(
            &states,
            cwd,
            cwd,
            context["pid"].as_u64().unwrap() as u32,
            rx,
            Arc::default(),
        );
        let stream = Path::new(&path).with_file_name("stream.bin");
        let mut offset = 0u64;
        let mut captures = Vec::new();
        for _ in 0..240 {
            tokio::time::sleep(Duration::from_millis(100)).await;
            let mut f = std::fs::File::open(&stream).unwrap();
            f.seek(SeekFrom::Start(offset)).unwrap();
            let mut bytes = Vec::new();
            f.read_to_end(&mut bytes).unwrap();
            offset += bytes.len() as u64;
            output.write(&bytes);
            if let Some(snapshot) = states
                .lock()
                .unwrap()
                .get("probe")
                .and_then(|s| s.snapshot())
            {
                captures.push(snapshot);
            }
        }
        tx.send(Some(0)).unwrap();
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert_eq!(
            states
                .lock()
                .unwrap()
                .get("probe")
                .unwrap()
                .snapshot()
                .unwrap()
                .observation
                .state,
            ObservationState::ProcessDead
        );
        std::fs::write(
            Path::new(&path).with_file_name("observer-captures.json"),
            serde_json::to_vec(&captures).unwrap(),
        )
        .unwrap();
        let current: Vec<_> = captures
            .iter()
            .filter(|s| s.is_current(s.observation.last_received_at_ms.unwrap()))
            .collect();
        assert!(
            current
                .iter()
                .any(|s| s.activity == SignalActivity::Working)
        );
        if context["expected"] == "error" {
            assert!(current.iter().any(|s| {
                s.outcome
                    .as_ref()
                    .is_some_and(|o| o.kind == OutcomeKind::Errored)
            }));
        } else {
            let kind = if context["expected"] == "question" {
                AttentionKind::Question
            } else {
                AttentionKind::Permission
            };
            assert!(
                current
                    .iter()
                    .any(|s| s.pending_attention.iter().any(|p| p.kind == kind
                        && p.request_id
                            == format!("call_native_{}", context["expected"].as_str().unwrap())))
            );
            assert!(current.iter().any(|s| {
                s.pending_attention.is_empty()
                    && s.outcome
                        .as_ref()
                        .is_some_and(|o| o.kind == OutcomeKind::Completed)
            }));
        }
    }
}
