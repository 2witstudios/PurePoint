//! Read-only native interactive state joined to root transcript identities.
use super::{LaunchState, NativeObserver, Prepared, install, now_ms};
use pu_core::signals::*;
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};
use std::io::{Read, Seek, SeekFrom};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::Path;
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

pub(super) struct Observer {
    command: String,
    env: Vec<(String, String)>,
}

pub(super) fn prepare(
    project: &str,
    agent: &str,
    worktree: Option<String>,
    session: &str,
    command: &str,
    env: &[(String, String)],
) -> Option<Prepared> {
    if !crate::read_source::valid_session_id(session) {
        return None;
    }
    let launch = pu_core::id::session_id();
    let artifacts = install::Artifacts::new(Path::new(project), &launch).ok()?;
    let socket = tokio::net::UnixDatagram::bind(artifacts.socket_directory.join("s")).ok()?;
    Some(Prepared {
        socket,
        token: String::new(),
        tool: "claude".into(),
        version: "2.1.300".into(),
        state: Arc::new(LaunchState {
            reducer: Mutex::new(None),
            identity: Mutex::new(SignalIdentity {
                agent_id: agent.into(),
                worktree_id: worktree,
                launch_id: launch,
                generation: 1,
                native_session_id: session.into(),
            }),
            native_snapshot: Mutex::new(None),
            macos_qualification: None,
            _artifacts: artifacts,
        }),
        output: None,
        observer: Some(NativeObserver::Claude(Observer {
            command: command.into(),
            env: env.to_vec(),
        })),
    })
}

impl Observer {
    async fn output(&self, cwd: &str, args: &[&str]) -> Option<Vec<u8>> {
        use tokio::io::AsyncReadExt;
        let mut child = tokio::process::Command::new(&self.command)
            .args(args)
            .env_clear()
            .envs(self.env.iter().map(|(k, v)| (k, v)))
            .current_dir(cwd)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .ok()?;
        let mut data = Vec::new();
        child
            .stdout
            .take()?
            .take(1024 * 1024 + 1)
            .read_to_end(&mut data)
            .await
            .ok()?;
        if data.len() > 1024 * 1024 || !child.wait().await.ok()?.success() {
            return None;
        }
        Some(data)
    }

    async fn row(&self, cwd: &str, pid: u32, session: &str) -> Option<Value> {
        let data = self.output(cwd, &["agents", "--json"]).await?;
        let rows: Value = serde_json::from_slice(&data).ok()?;
        let matching: Vec<_> = rows
            .as_array()?
            .iter()
            .filter(|r| r["pid"].as_u64() == Some(u64::from(pid)))
            .collect();
        if matching.len() != 1 {
            return None;
        }
        let row = matching[0];
        if row["cwd"].as_str() != Some(cwd)
            || row["sessionId"].as_str() != Some(session)
            || row["kind"] != "interactive"
            || row["startedAt"].as_i64().is_none()
        {
            return None;
        }
        Some(row.clone())
    }

    pub(super) fn activate(
        self,
        state: Weak<LaunchState>,
        project: String,
        cwd: String,
        pid: u32,
        mut exit: tokio::sync::watch::Receiver<Option<i32>>,
        channels: Arc<tokio::sync::Mutex<HashMap<String, tokio::sync::broadcast::Sender<()>>>>,
    ) {
        tokio::spawn(async move {
            let version =
                tokio::time::timeout(Duration::from_secs(3), self.output(&cwd, &["--version"]))
                    .await;
            if !version.ok().flatten().is_some_and(|bytes| {
                String::from_utf8_lossy(&bytes).split_whitespace().next() == Some("2.1.300")
            }) {
                return;
            }
            let mut transcript = Transcript::default();
            let mut started = None;
            let mut interval = tokio::time::interval(Duration::from_secs(1));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                tokio::select! { _=interval.tick()=>{}, _=exit.changed()=>{} }
                let Some(state) = state.upgrade() else { break };
                let dead = exit.borrow().is_some() || exit.has_changed().is_err();
                let identity = state.identity.lock().unwrap().clone();
                let poll = async {
                    let before = self.row(&cwd, pid, &identity.native_session_id).await?;
                    let epoch = before["startedAt"].as_i64()?;
                    if started.is_some_and(|old| old != epoch) {
                        return None;
                    }
                    started = Some(epoch);
                    let binding = crate::read_source::resolve_exact(
                        pu_core::read::Runtime::Claude,
                        &identity.native_session_id,
                        &cwd,
                        &self.env,
                    )
                    .ok()?;
                    let transcript_path = binding.path.clone();
                    let id = identity.clone();
                    let dir = cwd.clone();
                    let old = std::mem::take(&mut transcript);
                    let (next, valid) = tokio::task::spawn_blocking(move || {
                        let mut next = old;
                        let valid = next.read(&binding.path, &id, &dir).is_some();
                        (next, valid)
                    })
                    .await
                    .ok()?;
                    transcript = next;
                    if !valid {
                        return None;
                    }
                    let after = self.row(&cwd, pid, &identity.native_session_id).await?;
                    if before != after || !transcript.unchanged(&transcript_path) {
                        return None;
                    }
                    transcript.snapshot(&identity, &after, now_ms())
                };
                let result = if dead {
                    None
                } else {
                    tokio::time::timeout(Duration::from_secs(5), poll)
                        .await
                        .ok()
                        .flatten()
                };
                let dead = dead || exit.borrow().is_some() || exit.has_changed().is_err();
                {
                    let mut current = state.native_snapshot.lock().unwrap();
                    if !dead && let Some(snapshot) = result {
                        *current = Some(snapshot);
                    } else if let Some(snapshot) = current.as_mut() {
                        snapshot.observation.state = if dead {
                            ObservationState::ProcessDead
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

#[derive(Default)]
struct Transcript {
    poisoned: bool,
    offset: u64,
    inode: Option<(u64, u64)>,
    partial: Vec<u8>,
    turn: Option<String>,
    tools: BTreeMap<String, bool>,
    outcome: Option<SignalOutcome>,
}

impl Transcript {
    fn read(&mut self, path: &Path, identity: &SignalIdentity, cwd: &str) -> Option<()> {
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
            .map(|n| n + 1)
            .unwrap_or(0);
        let complete: Vec<u8> = self.partial.drain(..end).collect();
        for line in complete.split(|b| *b == b'\n').filter(|l| !l.is_empty()) {
            if line.len() > 1024 * 1024 {
                return None;
            }
            self.record(&serde_json::from_slice::<Value>(line).ok()?, identity, cwd)?;
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

    fn record(&mut self, v: &Value, identity: &SignalIdentity, cwd: &str) -> Option<()> {
        if !matches!(v["type"].as_str(), Some("user" | "assistant")) {
            return Some(());
        }
        if v["isSidechain"] == true {
            return Some(());
        }
        if v["sessionId"].as_str() != Some(&identity.native_session_id)
            || v["cwd"].as_str() != Some(cwd)
            || v["version"] != "2.1.300"
        {
            return None;
        }
        let uuid = v["uuid"]
            .as_str()
            .filter(|s| crate::read_source::valid_session_id(s))?;
        let message = &v["message"];
        if v["type"] == "user" {
            let results: Vec<_> = message["content"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|c| c["type"] == "tool_result")
                .collect();
            if !results.is_empty() {
                for result in results {
                    self.tools.remove(result["tool_use_id"].as_str()?);
                }
            } else if v["isMeta"] != true {
                self.turn = Some(uuid.into());
                self.tools.clear();
                self.outcome = None;
            }
        } else {
            for content in message["content"].as_array().into_iter().flatten() {
                if content["type"] == "tool_use" {
                    let id = content["id"]
                        .as_str()
                        .filter(|s| crate::read_source::valid_session_id(s))?;
                    if self.tools.len() >= 64 {
                        return None;
                    }
                    self.tools
                        .insert(id.into(), content["name"] == "AskUserQuestion");
                    self.outcome = None;
                }
            }
            let kind = if v["isApiErrorMessage"] == true {
                Some(OutcomeKind::Errored)
            } else if message["stop_reason"] == "end_turn" {
                Some(OutcomeKind::Completed)
            } else {
                None
            };
            if let Some(kind) = kind {
                self.outcome = Some(SignalOutcome {
                    kind,
                    event_id: format!("claude-message:{uuid}"),
                    turn_id: self.turn.clone(),
                    confidence: SignalConfidence::Authoritative,
                });
            }
        }
        Some(())
    }

    fn snapshot(&self, identity: &SignalIdentity, row: &Value, now: i64) -> Option<SignalSnapshot> {
        let status = row["status"].as_str()?;
        let mut pending_attention = Vec::new();
        let activity = match status {
            "busy" => SignalActivity::Working,
            "idle" => SignalActivity::Idle,
            "waiting" => {
                let question = match row["waitingFor"].as_str()? {
                    "input needed" => true,
                    "permission prompt" => false,
                    _ => return None,
                };
                // The native registry exposes a category, not a request ID. Only
                // join when exactly one outstanding root tool can own that UI.
                if self.tools.len() != 1 {
                    return None;
                }
                let (id, is_question) = self.tools.iter().next()?;
                if *is_question != question {
                    return None;
                }
                pending_attention.push(PendingAttention {
                    request_id: id.clone(),
                    kind: if question {
                        AttentionKind::Question
                    } else {
                        AttentionKind::Permission
                    },
                    event_id: format!("claude-request:{id}"),
                    turn_id: self.turn.clone(),
                    confidence: SignalConfidence::Authoritative,
                    actionability: AttentionActionability::default(),
                });
                SignalActivity::Working
            }
            _ => return None,
        };
        Some(SignalSnapshot {
            schema_version: 1,
            identity: identity.clone(),
            activity,
            pending_attention,
            outcome: if status == "idle" {
                self.outcome.clone()
            } else {
                None
            },
            observation: SignalObservation {
                state: ObservationState::Current,
                last_received_at_ms: Some(now),
                expires_at_ms: Some(now + 30_000),
            },
            evidence: Some(SignalEvidence {
                tool: "claude".into(),
                tool_version: "2.1.300".into(),
                adapter_version: "2".into(),
                confidence: SignalConfidence::Authoritative,
                event_id: format!(
                    "claude-state:{}:{}:{}",
                    identity.native_session_id,
                    self.turn.as_deref().unwrap_or("unstarted"),
                    status
                ),
            }),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn identity() -> SignalIdentity {
        SignalIdentity {
            agent_id: "agent".into(),
            worktree_id: None,
            launch_id: "launch".into(),
            generation: 1,
            native_session_id: "native".into(),
        }
    }
    fn record(kind: &str, id: &str, message: Value) -> Value {
        json!({"type":kind,"uuid":id,"sessionId":"native","cwd":"/owned","version":"2.1.300","isSidechain":false,"message":message})
    }
    #[test]
    fn exact_root_request_identity_and_response_are_retained() {
        let mut t = Transcript::default();
        let id = identity();
        t.record(
            &record("user", "turn", json!({"content":"hello"})),
            &id,
            "/owned",
        )
        .unwrap();
        t.record(&record("assistant","a",json!({"content":[{"type":"tool_use","id":"request","name":"AskUserQuestion","input":{"secret":"not retained"}}],"stop_reason":"tool_use"})),&id,"/owned").unwrap();
        assert_eq!(t.turn.as_deref(), Some("turn"));
        assert_eq!(t.tools.get("request"), Some(&true));
        t.record(
            &record(
                "user",
                "answer",
                json!({"content":[{"type":"tool_result","tool_use_id":"request"}]}),
            ),
            &id,
            "/owned",
        )
        .unwrap();
        assert!(t.tools.is_empty());
        assert_eq!(t.turn.as_deref(), Some("turn"));
    }
    #[test]
    fn terminal_error_is_structural_and_next_prompt_clears_old_outcome() {
        let mut t = Transcript::default();
        let id = identity();
        t.record(
            &record("user", "turn", json!({"content":"hello"})),
            &id,
            "/owned",
        )
        .unwrap();
        let mut error = record(
            "assistant",
            "error",
            json!({"content":[],"stop_reason":"stop_sequence"}),
        );
        error["isApiErrorMessage"] = json!(true);
        t.record(&error, &id, "/owned").unwrap();
        assert_eq!(t.outcome.as_ref().unwrap().kind, OutcomeKind::Errored);
        t.record(
            &record("user", "next", json!({"content":"next"})),
            &id,
            "/owned",
        )
        .unwrap();
        assert!(t.outcome.is_none());
        t.record(
            &record(
                "assistant",
                "done",
                json!({"content":[],"stop_reason":"end_turn"}),
            ),
            &id,
            "/owned",
        )
        .unwrap();
        assert_eq!(t.outcome.as_ref().unwrap().kind, OutcomeKind::Completed);
    }
    #[test]
    fn foreign_child_and_future_version_cannot_change_root_facts() {
        let id = identity();
        let mut t = Transcript::default();
        for (field, value) in [
            ("sessionId", json!("foreign")),
            ("cwd", json!("/foreign")),
            ("version", json!("9.0.0")),
            ("isSidechain", json!(true)),
        ] {
            let mut v = record("user", "foreign", json!({"content":"ignored"}));
            v[field] = value;
            let _ = t.record(&v, &id, "/owned");
            assert!(t.turn.is_none());
        }
    }
    #[test]
    fn native_wait_category_requires_one_exact_root_request() {
        let mut t = Transcript::default();
        let id = identity();
        t.tools.insert("question".into(), true);
        let waiting = json!({"status":"waiting","waitingFor":"input needed"});
        let snapshot = t.snapshot(&id, &waiting, 100).unwrap();
        assert_eq!(snapshot.pending_attention[0].request_id, "question");
        assert_eq!(snapshot.pending_attention[0].kind, AttentionKind::Question);
        assert!(snapshot.is_current(101));
        assert!(
            t.snapshot(
                &id,
                &json!({"status":"waiting","waitingFor":"permission prompt"}),
                100
            )
            .is_none()
        );
        t.tools.insert("other".into(), false);
        assert!(t.snapshot(&id, &waiting, 100).is_none());
        assert!(
            t.snapshot(&id, &json!({"status":"busy"}), 100)
                .unwrap()
                .pending_attention
                .is_empty()
        );
        t.tools.clear();
        t.tools.insert("permission".into(), false);
        assert_eq!(
            t.snapshot(
                &id,
                &json!({"status":"waiting","waitingFor":"permission prompt"}),
                100
            )
            .unwrap()
            .pending_attention[0]
                .kind,
            AttentionKind::Permission
        );
    }

    #[test]
    fn incremental_reader_waits_for_complete_lines_and_latches_history_loss() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("transcript");
        let bytes =
            serde_json::to_vec(&record("user", "turn", json!({"content":"hello"}))).unwrap();
        std::fs::write(&path, &bytes[..10]).unwrap();
        let mut t = Transcript::default();
        let id = identity();
        t.read(&path, &id, "/owned").unwrap();
        assert!(t.turn.is_none());
        std::fs::write(&path, [bytes.as_slice(), b"\n"].concat()).unwrap();
        t.read(&path, &id, "/owned").unwrap();
        assert_eq!(t.turn.as_deref(), Some("turn"));
        std::fs::write(&path, b"").unwrap();
        assert!(t.read(&path, &id, "/owned").is_none());
        std::fs::write(&path, [bytes.as_slice(), b"\n"].concat()).unwrap();
        assert!(t.read(&path, &id, "/owned").is_none());
    }

    /// Driven only by an isolated native CLI fixture; never reads real user config.
    #[tokio::test]
    #[ignore = "requires the sandbox native CLI probe context"]
    async fn native_interactive_observer_probe() {
        let path =
            std::env::var("PUREPOINT_CLAUDE_PROBE_CONTEXT").expect("sandbox context required");
        let context: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        let cwd = context["cwd"].as_str().unwrap();
        let env: Vec<(String, String)> = serde_json::from_value(context["env"].clone()).unwrap();
        let prepared = prepare(
            cwd,
            "probe",
            None,
            context["session"].as_str().unwrap(),
            context["command"].as_str().unwrap(),
            &env,
        )
        .unwrap();
        let states = Arc::new(Mutex::new(HashMap::new()));
        let (exit_tx, exit_rx) = tokio::sync::watch::channel(None);
        prepared.activate(
            &states,
            cwd,
            cwd,
            context["pid"].as_u64().unwrap() as u32,
            exit_rx,
            Arc::new(tokio::sync::Mutex::new(HashMap::new())),
        );
        let mut captures = Vec::new();
        for _ in 0..180 {
            tokio::time::sleep(Duration::from_millis(100)).await;
            if let Some(snapshot) = states
                .lock()
                .unwrap()
                .get("probe")
                .and_then(|s| s.snapshot())
            {
                captures.push(snapshot);
            }
        }
        exit_tx.send(Some(0)).unwrap();
        tokio::time::sleep(Duration::from_millis(200)).await;
        let final_snapshot = states
            .lock()
            .unwrap()
            .get("probe")
            .unwrap()
            .snapshot()
            .unwrap();
        assert_eq!(
            final_snapshot.observation.state,
            ObservationState::ProcessDead
        );
        std::fs::write(
            Path::new(&path).with_file_name("observer-captures.json"),
            serde_json::to_vec(&captures).unwrap(),
        )
        .unwrap();
        assert!(
            captures
                .iter()
                .any(|s| s.activity == SignalActivity::Working
                    && s.is_current(s.observation.last_received_at_ms.unwrap()))
        );
        let expected = context["expected"].as_str().unwrap();
        if expected == "error" {
            assert!(captures.iter().any(|s| {
                s.outcome
                    .as_ref()
                    .is_some_and(|o| o.kind == OutcomeKind::Errored)
            }));
        } else {
            let kind = if expected == "question" {
                AttentionKind::Question
            } else {
                AttentionKind::Permission
            };
            assert!(
                captures
                    .iter()
                    .any(|s| s.pending_attention.iter().any(|p| p.kind == kind))
            );
            assert!(captures.iter().any(|s| {
                s.outcome
                    .as_ref()
                    .is_some_and(|o| o.kind == OutcomeKind::Completed)
                    && s.pending_attention.is_empty()
            }));
        }
    }
}
