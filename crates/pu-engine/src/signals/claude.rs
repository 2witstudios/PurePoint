//! Read-only native interactive state joined to root transcript identities.
use super::{LaunchState, NativeObserver, Prepared, install, now_ms};
use pu_core::signals::*;
use serde_json::Value;
use std::cmp::Reverse;
use std::collections::{BTreeMap, BinaryHeap, HashMap, HashSet};
use std::io::{Read, Seek, SeekFrom};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::Path;
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

pub(super) struct Observer {
    not_before_ms: i64,
    env: Vec<(String, String)>,
    initial_transcript: Box<Transcript>,
}

pub(super) async fn prepare_launch(
    project: &str,
    cwd: &str,
    agent: &str,
    worktree: Option<String>,
    session: &str,
    env: &[(String, String)],
    resuming: bool,
) -> Option<Prepared> {
    let mut prepared = prepare(project, agent, worktree, session, env)?;
    if resuming {
        let (cwd, session, env) = (cwd.to_owned(), session.to_owned(), env.to_vec());
        let boundary = tokio::time::timeout(
            Duration::from_millis(500),
            tokio::task::spawn_blocking(move || {
                let binding = crate::read_source::resolve_exact(
                    pu_core::read::Runtime::Claude,
                    &session,
                    &cwd,
                    &env,
                )
                .ok()?;
                super::native_file_boundary(&binding.path)
            }),
        )
        .await
        .ok()?
        .ok()??;
        let Some(NativeObserver::Claude(observer)) = prepared.observer.as_mut() else {
            return None;
        };
        observer.initial_transcript = Box::new(Transcript {
            materialized: true,
            offset: boundary.offset,
            observed_length: boundary.offset,
            inode: Some(boundary.inode),
            ..Transcript::default()
        });
    }
    Some(prepared)
}

pub(super) fn prepare(
    project: &str,
    agent: &str,
    worktree: Option<String>,
    session: &str,
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
            not_before_ms: now_ms(),
            env: env.to_vec(),
            initial_transcript: Box::default(),
        })),
    })
}

// Match the native registry namespace, not merely the operating-system name.
// The direct PTY child inherits this daemon's PID namespace. In particular the
// Linux native format includes both the machine ID and PID-namespace link.
async fn native_pid_domain() -> Option<&'static str> {
    static DOMAIN: tokio::sync::OnceCell<String> = tokio::sync::OnceCell::const_new();
    DOMAIN
        .get_or_try_init(|| async {
            #[cfg(target_os = "macos")]
            {
                Ok::<String, ()>("darwin".into())
            }
            #[cfg(target_os = "linux")]
            {
                let (machine, namespace) = tokio::join!(
                    tokio::fs::read_to_string("/etc/machine-id"),
                    tokio::fs::read_link("/proc/self/ns/pid")
                );
                let machine = machine.map_err(|_| ())?;
                let namespace = namespace.map_err(|_| ())?;
                let machine = machine.trim();
                if machine.is_empty() {
                    return Err(());
                }
                Ok::<String, ()>(format!("linux:{machine}:{}", namespace.to_str().ok_or(())?))
            }
            #[cfg(not(any(target_os = "macos", target_os = "linux")))]
            {
                Err::<String, ()>(())
            }
        })
        .await
        .ok()
        .map(String::as_str)
}

impl Observer {
    async fn row(&self, cwd: &str, pid: u32, session: &str) -> Option<Value> {
        use tokio::io::AsyncReadExt;
        // The native interactive registry is stored per process.
        // Read only the owned PID: no list subprocesses or all-agent scans per
        // observer, and no cross-profile shared cache that can mix identities.
        let root =
            crate::read_source::native_root(pu_core::read::Runtime::Claude, &self.env).ok()?;
        let file = tokio::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(root.join("sessions").join(format!("{pid}.json")))
            .await
            .ok()?;
        let meta = file.metadata().await.ok()?;
        if !meta.is_file()
            || meta.uid() != unsafe { libc::geteuid() }
            || meta.mode() & 0o022 != 0
            || meta.len() > 16 * 1024
        {
            return None;
        }
        let mut data = Vec::new();
        file.take(16 * 1024 + 1).read_to_end(&mut data).await.ok()?;
        if data.len() > 16 * 1024 {
            return None;
        }
        let row: Value = serde_json::from_slice(&data).ok()?;
        let domain = native_pid_domain().await?;
        if row["pid"].as_u64() != Some(u64::from(pid))
            || row["cwd"].as_str() != Some(cwd)
            || row["sessionId"].as_str() != Some(session)
            || row["kind"] != "interactive"
            || row["version"] != "2.1.300"
            || row["peerProtocol"] != 1
            || row["entrypoint"] != "cli"
            || row["pidDomain"] != domain
            || row["startedAt"]
                .as_i64()
                .is_none_or(|started| started < self.not_before_ms)
        {
            return None;
        }
        Some(row)
    }

    pub(super) fn activate(
        mut self,
        state: Weak<LaunchState>,
        project: String,
        cwd: String,
        pid: u32,
        mut exit: tokio::sync::watch::Receiver<Option<i32>>,
        channels: Arc<tokio::sync::Mutex<HashMap<String, tokio::sync::broadcast::Sender<()>>>>,
    ) {
        tokio::spawn(async move {
            let mut transcript = *std::mem::take(&mut self.initial_transcript);
            let mut pending_read: Option<tokio::task::JoinHandle<(Transcript, bool)>> = None;
            let mut started = None;
            let mut publication = super::NativePublication::default();
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
                    if pending_read.is_none()
                        && transcript.initial_idle_without_transcript(&binding.path, &before)
                    {
                        let after = self.row(&cwd, pid, &identity.native_session_id).await?;
                        return if before == after {
                            transcript.snapshot(&identity, &after, now_ms())
                        } else {
                            None
                        };
                    }
                    let transcript_path = binding.path.clone();
                    let id = identity.clone();
                    let dir = cwd.clone();
                    // Keep ownership of an in-flight blocking read across poll timeouts.
                    // Dropping its handle would lose settled/replay state and allow a
                    // second reader to replay the file from the beginning.
                    if pending_read.is_none() {
                        let old = std::mem::take(&mut transcript);
                        pending_read = Some(tokio::task::spawn_blocking(move || {
                            let mut next = old;
                            let valid = next.read(&binding.path, &id, &dir).is_some();
                            (next, valid)
                        }));
                    }
                    let read = pending_read.as_mut()?.await;
                    pending_read = None;
                    let (next, valid) = match read {
                        Ok(result) => result,
                        Err(_) => {
                            transcript.poisoned = true;
                            return None;
                        }
                    };
                    transcript = next;
                    if !valid {
                        return None;
                    }
                    let after = self.row(&cwd, pid, &identity.native_session_id).await?;
                    if before != after || !transcript.observation_ready(&transcript_path) {
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
                if publication.should_publish(state.snapshot(), tokio::time::Instant::now())
                    && let Some(tx) = channels.lock().await.get(&project)
                {
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
    materialized: bool,
    offset: u64,
    observed_length: u64,
    inode: Option<(u64, u64)>,
    partial: Vec<u8>,
    turn: Option<String>,
    settled: bool,
    working_seen: bool,
    lineage: HashSet<String>,
    pending_records: BTreeMap<String, DeferredRecord>,
    waiting_for_parent: HashMap<String, Vec<String>>,
    record_sequence: u64,
    closed_turns: HashSet<String>,
    tools: BTreeMap<String, bool>,
    outcome: Option<SignalOutcome>,
}

struct DeferredRecord {
    parent: String,
    order: u64,
    facts: RecordFacts,
}

struct RecordFacts {
    tools: Option<Vec<(String, bool)>>,
    results: Option<Vec<String>>,
    outcome: Option<OutcomeKind>,
}

impl RecordFacts {
    fn from_record(record: &Value) -> Self {
        let parts: Vec<_> = record["message"]["content"]
            .as_array()
            .into_iter()
            .flatten()
            .collect();
        let tools = if record["type"] == "assistant" {
            let entries: Vec<_> = parts
                .iter()
                .filter(|part| part["type"] == "tool_use")
                .collect();
            if entries.len() > 64 {
                None
            } else {
                entries
                    .into_iter()
                    .map(|part| {
                        Some((
                            part["id"]
                                .as_str()
                                .filter(|id| crate::read_source::valid_session_id(id))?
                                .into(),
                            part["name"] == "AskUserQuestion",
                        ))
                    })
                    .collect()
            }
        } else {
            Some(Vec::new())
        };
        let results = if record["type"] == "user" {
            let entries: Vec<_> = parts
                .iter()
                .filter(|part| part["type"] == "tool_result")
                .collect();
            if entries.len() > 64 {
                None
            } else {
                entries
                    .into_iter()
                    .map(|part| {
                        Some(
                            part["tool_use_id"]
                                .as_str()
                                .filter(|id| crate::read_source::valid_session_id(id))?
                                .into(),
                        )
                    })
                    .collect()
            }
        } else {
            Some(Vec::new())
        };
        let outcome = if record["type"] != "assistant" {
            None
        } else if record["isApiErrorMessage"] == true {
            Some(OutcomeKind::Errored)
        } else if record["message"]["stop_reason"] == "end_turn" {
            Some(OutcomeKind::Completed)
        } else {
            None
        };
        Self {
            tools,
            results,
            outcome,
        }
    }
}

impl Transcript {
    fn read(&mut self, path: &Path, identity: &SignalIdentity, cwd: &str) -> Option<()> {
        self.materialized |= std::fs::symlink_metadata(path).is_ok();
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
            || meta.len() < self.observed_length
            || self.inode.is_some_and(|old| old != inode)
        {
            return None;
        }
        self.inode = Some(inode);
        self.observed_length = meta.len();
        file.seek(SeekFrom::Start(self.offset)).ok()?;
        let mut bytes = Vec::new();
        file.take(8 * 1024 * 1024).read_to_end(&mut bytes).ok()?;
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
            if line.len() > super::MAX_NATIVE_RECORD_BYTES {
                return None;
            }
            self.record(&serde_json::from_slice::<Value>(line).ok()?, identity, cwd)?;
        }
        if self.partial.len() > super::MAX_NATIVE_RECORD_BYTES {
            return None;
        }
        self.poisoned = false;
        Some(())
    }

    fn initial_idle_without_transcript(&self, path: &Path, row: &Value) -> bool {
        !self.materialized
            && self.inode.is_none()
            && !self.poisoned
            && row["status"] == "idle"
            && std::fs::symlink_metadata(path)
                .is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound)
    }

    fn observation_ready(&mut self, path: &Path) -> bool {
        // A read capped by the per-poll budget leaves complete records unread.
        if self.poisoned || self.offset < self.observed_length {
            return false;
        }
        let Some(length) =
            super::native_prefix_length(path, self.inode, self.offset, self.observed_length)
        else {
            return false;
        };
        self.observed_length = length;
        true
    }

    #[cfg(test)]
    fn unchanged(&self, path: &Path) -> bool {
        self.partial.is_empty()
            && std::fs::symlink_metadata(path).is_ok_and(|m| {
                m.is_file() && Some((m.dev(), m.ino())) == self.inode && m.len() == self.offset
            })
    }

    fn record(&mut self, v: &Value, identity: &SignalIdentity, cwd: &str) -> Option<()> {
        let content_record = matches!(v["type"].as_str(), Some("user" | "assistant"));
        if v["isSidechain"] == true {
            return Some(());
        }
        if v["sessionId"].as_str() != Some(&identity.native_session_id)
            || v["cwd"].as_str() != Some(cwd)
            || v["version"] != "2.1.300"
        {
            return if content_record { None } else { Some(()) };
        }
        let Some(uuid) = v["uuid"]
            .as_str()
            .filter(|s| crate::read_source::valid_session_id(s))
        else {
            return if content_record { None } else { Some(()) };
        };
        let message = &v["message"];
        let results: Vec<_> = message["content"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|c| c["type"] == "tool_result")
            .collect();
        if v["type"] == "user"
            && results.is_empty()
            && v["isMeta"] != true
            && v["isCompactSummary"] != true
        {
            if self.turn.as_deref() == Some(uuid) || self.closed_turns.contains(uuid) {
                return Some(());
            }
            if let Some(previous) = self.turn.clone() {
                if !self.closed_turns.contains(&previous) && self.closed_turns.len() >= 512 {
                    return None;
                }
                self.closed_turns.insert(previous);
            }
            // The real native CLI writes this root user record only after its
            // rejectable submit hooks accept. A busy registry alone is intent.
            self.turn = Some(uuid.into());
            self.settled = false;
            self.working_seen = false;
            self.tools.clear();
            self.outcome = None;
            self.discard_waiting(self.lineage.iter().cloned().collect());
            self.lineage.clear();
            if self.pending_records.len() >= 4096 {
                return None;
            }
            self.lineage.insert(uuid.into());
            return self.promote_waiting(uuid);
        }
        if self.lineage.contains(uuid) || self.pending_records.contains_key(uuid) {
            return Some(());
        }
        let Some(parent) = v["parentUuid"]
            .as_str()
            .filter(|id| crate::read_source::valid_session_id(id))
        else {
            return Some(());
        };
        if self.closed_turns.contains(parent) {
            self.discard_waiting(vec![uuid.into()]);
            return Some(());
        }
        if self.lineage.len() + self.pending_records.len() >= 4096 {
            return None;
        }
        let facts = RecordFacts::from_record(v);
        if !self.lineage.contains(parent) {
            self.record_sequence = self.record_sequence.checked_add(1)?;
            self.pending_records.insert(
                uuid.into(),
                DeferredRecord {
                    parent: parent.into(),
                    order: self.record_sequence,
                    facts,
                },
            );
            self.waiting_for_parent
                .entry(parent.into())
                .or_default()
                .push(uuid.into());
            return Some(());
        }
        self.lineage.insert(uuid.into());
        self.apply_facts(uuid, facts)?;
        self.promote_waiting(uuid)
    }

    fn apply_facts(&mut self, uuid: &str, facts: RecordFacts) -> Option<()> {
        for id in facts.results? {
            self.tools.remove(&id);
        }
        if !self.settled {
            for (id, question) in facts.tools? {
                if self.tools.len() >= 64 && !self.tools.contains_key(&id) {
                    return None;
                }
                self.tools.insert(id, question);
                self.outcome = None;
            }
        }
        if let Some(kind) = facts.outcome
            && (!self.settled || self.outcome.is_none())
        {
            self.outcome = Some(SignalOutcome {
                kind,
                event_id: format!("claude-message:{uuid}"),
                turn_id: self.turn.clone(),
                confidence: SignalConfidence::Authoritative,
            });
        }
        Some(())
    }

    fn promote_waiting(&mut self, parent: &str) -> Option<()> {
        let mut ready = BinaryHeap::new();
        if let Some(children) = self.waiting_for_parent.remove(parent) {
            for id in children {
                if let Some(record) = self.pending_records.get(&id) {
                    ready.push(Reverse((record.order, id)));
                }
            }
        }
        while let Some(Reverse((_, id))) = ready.pop() {
            let record = self.pending_records.remove(&id)?;
            if !self.lineage.contains(&record.parent) {
                return None;
            }
            self.lineage.insert(id.clone());
            self.apply_facts(&id, record.facts)?;
            if let Some(children) = self.waiting_for_parent.remove(&id) {
                for child in children {
                    if let Some(record) = self.pending_records.get(&child) {
                        ready.push(Reverse((record.order, child)));
                    }
                }
            }
        }
        Some(())
    }

    fn discard_waiting(&mut self, mut retired: Vec<String>) {
        while let Some(parent) = retired.pop() {
            if let Some(children) = self.waiting_for_parent.remove(&parent) {
                for id in children {
                    self.pending_records.remove(&id);
                    retired.push(id);
                }
            }
        }
    }

    fn snapshot(
        &mut self,
        identity: &SignalIdentity,
        row: &Value,
        now: i64,
    ) -> Option<SignalSnapshot> {
        let status = row["status"].as_str()?;
        let mut pending_attention = Vec::new();
        let activity = match status {
            "busy" => {
                if self.turn.is_none() || self.settled {
                    return None;
                }
                self.working_seen = true;
                SignalActivity::Working
            }
            "idle" => {
                if self.working_seen || self.outcome.is_some() {
                    self.settled = true;
                    self.tools.clear();
                }
                SignalActivity::Idle
            }
            "waiting" => {
                let question = match row["waitingFor"].as_str()? {
                    "input needed" => true,
                    "permission prompt" => false,
                    _ => return None,
                };
                // The native registry exposes a category, not a request ID. Only
                // join when exactly one outstanding root tool can own that UI,
                // and not while a record that may resolve it is still arriving.
                if self.tools.len() != 1 || !self.partial.is_empty() {
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
                    event_id: format!(
                        "claude-request:{}:{id}",
                        self.turn.as_deref().unwrap_or("ui")
                    ),
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
    #[test]
    fn initial_registry_idle_does_not_require_a_transcript_or_hide_a_lost_one() {
        let temp = tempfile::tempdir().unwrap();
        let missing = temp.path().join("not-yet-created.jsonl");
        let mut transcript = Transcript::default();
        assert!(transcript.initial_idle_without_transcript(&missing, &json!({"status":"idle"})));
        for status in ["busy", "waiting", "future"] {
            assert!(
                !transcript.initial_idle_without_transcript(&missing, &json!({"status":status}))
            );
        }
        transcript.inode = Some((1, 2));
        assert!(!transcript.initial_idle_without_transcript(&missing, &json!({"status":"idle"})));
        transcript.inode = None;
        transcript.poisoned = true;
        assert!(!transcript.initial_idle_without_transcript(&missing, &json!({"status":"idle"})));
        transcript.poisoned = false;
        std::fs::write(&missing, "").unwrap();
        assert!(!transcript.initial_idle_without_transcript(&missing, &json!({"status":"idle"})));
        std::fs::remove_file(&missing).unwrap();
        std::os::unix::fs::symlink(temp.path().join("absent-target"), &missing).unwrap();
        assert!(transcript.read(&missing, &identity(), "/owned").is_none());
        std::fs::remove_file(&missing).unwrap();
        assert!(
            !transcript.initial_idle_without_transcript(&missing, &json!({"status":"idle"})),
            "An invalid source that disappears must not become initial idle"
        );
    }
    #[test]
    fn waiting_request_is_not_joined_while_a_record_is_still_arriving() {
        let mut transcript = Transcript {
            turn: Some("turn".into()),
            ..Transcript::default()
        };
        transcript.tools.insert("request".into(), true);
        let waiting = json!({"status":"waiting","waitingFor":"input needed"});
        transcript.partial = b"{\"type\":\"user\"".to_vec();
        assert!(transcript.snapshot(&identity(), &waiting, 100).is_none());
        transcript.partial.clear();
        assert_eq!(
            transcript
                .snapshot(&identity(), &waiting, 100)
                .unwrap()
                .pending_attention
                .len(),
            1
        );
    }
    #[test]
    fn backlog_beyond_read_budget_is_not_an_observation() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("transcript.jsonl");
        let line = format!("{{\"padding\":\"{}\"}}\n", "x".repeat(64 * 1024));
        std::fs::write(&path, line.repeat(160)).unwrap();
        // Simulate the read cursor after one capped read.
        let meta = std::fs::metadata(&path).unwrap();
        let mut transcript = Transcript {
            inode: Some((meta.dev(), meta.ino())),
            observed_length: meta.len(),
            offset: 8 * 1024 * 1024,
            ..Transcript::default()
        };
        assert!(!transcript.observation_ready(&path));
        transcript.offset = meta.len();
        assert!(transcript.observation_ready(&path));
    }
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
        let parent = if kind == "assistant" {
            json!("turn")
        } else if message["content"]
            .as_array()
            .is_some_and(|a| a.iter().any(|c| c["type"] == "tool_result"))
        {
            json!("a")
        } else {
            Value::Null
        };
        json!({"type":kind,"uuid":id,"parentUuid":parent,"sessionId":"native","cwd":"/owned","version":"2.1.300","isSidechain":false,"message":message})
    }
    #[test]
    fn given_large_native_image_result_should_resolve_the_request_and_preserve_completion() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("history");
        let user = record("user", "turn", json!({"content":"read image"}));
        let call = record(
            "assistant",
            "a",
            json!({"content":[{"type":"tool_use","id":"image-read","name":"Read"}]}),
        );
        let result = record(
            "user",
            "image-result",
            json!({"content":[{"type":"tool_result","tool_use_id":"image-read","content":[{"type":"image","source":{"type":"base64","media_type":"image/png","data":"A".repeat(10*1024*1024)}}]}]}),
        );
        let mut completed = record(
            "assistant",
            "end",
            json!({"stop_reason":"end_turn","content":[]}),
        );
        completed["parentUuid"] = json!("image-result");
        std::fs::write(&path, format!("{user}\n{call}\n{result}\n{completed}\n")).unwrap();
        let mut transcript = Transcript::default();
        transcript.read(&path, &identity(), "/owned").unwrap();
        assert!(!transcript.poisoned);
        assert!(!transcript.unchanged(&path));
        transcript.read(&path, &identity(), "/owned").unwrap();
        assert!(transcript.unchanged(&path));
        assert!(transcript.tools.is_empty());
        assert_eq!(transcript.outcome.unwrap().kind, OutcomeKind::Completed);
    }
    #[test]
    fn existing_large_history_is_consumed_in_bounded_batches() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("history");
        let line = format!(
            "{{\"type\":\"metadata\",\"padding\":\"{}\"}}\n",
            "x".repeat(1024)
        );
        std::fs::write(&path, line.repeat(9000)).unwrap();
        let mut reader = Transcript::default();
        reader.read(&path, &identity(), "/owned").unwrap();
        assert!(!reader.poisoned);
        assert!(!reader.unchanged(&path));
        reader.read(&path, &identity(), "/owned").unwrap();
        assert!(reader.unchanged(&path));
    }

    #[tokio::test]
    async fn given_resumed_claude_history_should_not_replay_outcomes_or_exhaust_new_launch_history()
    {
        use std::io::Write;
        for (turns, errored) in [(1, false), (1, true), (600, false)] {
            let root = tempfile::tempdir().unwrap();
            let folder = root.path().join("projects/-owned");
            std::fs::create_dir_all(&folder).unwrap();
            let path = folder.join("native.jsonl");
            let mut file = std::fs::File::create(&path).unwrap();
            for index in 0..turns {
                let id = format!("old{index}");
                writeln!(
                    file,
                    "{}",
                    record("user", &id, json!({"content":"old prompt"}))
                )
                .unwrap();
                let mut end = record(
                    "assistant",
                    &format!("end{index}"),
                    json!({"stop_reason":"end_turn","content":[]}),
                );
                end["parentUuid"] = json!(id);
                end["isApiErrorMessage"] = json!(errored);
                writeln!(file, "{end}").unwrap();
            }
            let env = vec![
                ("HOME".into(), root.path().to_string_lossy().into_owned()),
                (
                    "CLAUDE_CONFIG_DIR".into(),
                    root.path().to_string_lossy().into_owned(),
                ),
            ];
            let prepared = prepare_launch(
                root.path().to_str().unwrap(),
                "/owned",
                "agent",
                None,
                "native",
                &env,
                true,
            )
            .await
            .unwrap();
            let identity = prepared.state.identity.lock().unwrap().clone();
            let Some(NativeObserver::Claude(mut observer)) = prepared.observer else {
                panic!("missing Claude observer");
            };
            observer
                .initial_transcript
                .read(&path, &identity, "/owned")
                .unwrap();
            let idle = observer
                .initial_transcript
                .snapshot(&identity, &json!({"status":"idle"}), 100)
                .unwrap();
            assert!(
                idle.outcome.is_none(),
                "old completion must not move into a new launch"
            );
            assert!(observer.initial_transcript.turn.is_none());
            assert!(observer.initial_transcript.closed_turns.is_empty());
            writeln!(
                file,
                "{}",
                record("user", "new", json!({"content":"current prompt"}))
            )
            .unwrap();
            let mut end = record(
                "assistant",
                "current-end",
                json!({"stop_reason":"end_turn","content":[]}),
            );
            end["parentUuid"] = json!("new");
            writeln!(file, "{end}").unwrap();
            observer
                .initial_transcript
                .read(&path, &identity, "/owned")
                .unwrap();
            let current = observer
                .initial_transcript
                .snapshot(&identity, &json!({"status":"idle"}), 101)
                .unwrap();
            assert_eq!(current.outcome.unwrap().turn_id.as_deref(), Some("new"));
        }
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
        let mut done = record(
            "assistant",
            "done",
            json!({"content":[],"stop_reason":"end_turn"}),
        );
        done["parentUuid"] = json!("next");
        t.record(&done, &id, "/owned").unwrap();
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
        t.record(
            &record("user", "turn", json!({"content":"accepted"})),
            &id,
            "/owned",
        )
        .unwrap();
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

    #[test]
    fn rejectable_busy_cannot_start_or_revive_a_settled_turn() {
        let mut t = Transcript::default();
        let id = identity();
        let busy = json!({"status":"busy"});
        let idle = json!({"status":"idle"});
        assert!(t.snapshot(&id, &busy, 100).is_none());
        t.record(
            &record("user", "turn", json!({"content":"accepted"})),
            &id,
            "/owned",
        )
        .unwrap();
        let mut completed = record(
            "assistant",
            "done",
            json!({"content":[],"stop_reason":"end_turn"}),
        );
        completed["parentUuid"] = json!("turn");
        t.record(&completed, &id, "/owned").unwrap();
        assert!(t.snapshot(&id, &idle, 100).unwrap().outcome.is_some());
        assert!(t.snapshot(&id, &busy, 101).is_none());
        assert_eq!(
            t.snapshot(&id, &idle, 102)
                .unwrap()
                .outcome
                .unwrap()
                .event_id,
            "claude-message:done"
        );
    }
    #[test]
    fn delayed_old_branch_error_cannot_close_the_new_accepted_turn() {
        let mut t = Transcript::default();
        let id = identity();
        t.record(
            &record("user", "turn", json!({"content":"first"})),
            &id,
            "/owned",
        )
        .unwrap();
        t.record(
            &record("user", "next", json!({"content":"second"})),
            &id,
            "/owned",
        )
        .unwrap();
        let mut old = record("assistant", "late-error", json!({"content":[]}));
        old["parentUuid"] = json!("turn");
        old["isApiErrorMessage"] = json!(true);
        t.record(&old, &id, "/owned").unwrap();
        assert!(t.outcome.is_none());
        let mut current = record(
            "assistant",
            "new-done",
            json!({"content":[],"stop_reason":"end_turn"}),
        );
        current["parentUuid"] = json!("next");
        t.record(&current, &id, "/owned").unwrap();
        assert_eq!(t.outcome.as_ref().unwrap().turn_id.as_deref(), Some("next"));
    }

    #[test]
    fn closed_root_replays_and_capacity_noops_preserve_the_current_turn() {
        let mut t = Transcript::default();
        let id = identity();
        for index in 0..513 {
            t.record(
                &record(
                    "user",
                    &format!("root{index}"),
                    json!({"content":"accepted"}),
                ),
                &id,
                "/owned",
            )
            .unwrap();
        }
        assert_eq!(t.closed_turns.len(), 512);
        for root in ["root0", "root512"] {
            t.record(
                &record("user", root, json!({"content":"replay"})),
                &id,
                "/owned",
            )
            .unwrap();
        }
        assert_eq!(t.turn.as_deref(), Some("root512"));
        assert_eq!(t.closed_turns.len(), 512);
        assert!(
            t.record(
                &record("user", "overflow", json!({"content":"new"})),
                &id,
                "/owned"
            )
            .is_none()
        );
        assert_eq!(t.turn.as_deref(), Some("root512"));
    }
    #[test]
    fn delayed_terminal_flush_can_close_unknown_idle_without_reopening_settled_activity() {
        let mut t = Transcript::default();
        let id = identity();
        let busy = json!({"status":"busy"});
        let idle = json!({"status":"idle"});
        t.record(
            &record("user", "turn", json!({"content":"accepted"})),
            &id,
            "/owned",
        )
        .unwrap();
        t.snapshot(&id, &busy, 100).unwrap();
        assert!(t.snapshot(&id, &idle, 101).unwrap().outcome.is_none());
        let mut error = record("assistant", "late-flush", json!({"content":[]}));
        error["isApiErrorMessage"] = json!(true);
        t.record(&error, &id, "/owned").unwrap();
        assert_eq!(
            t.snapshot(&id, &idle, 102).unwrap().outcome.unwrap().kind,
            OutcomeKind::Errored
        );
        assert!(t.snapshot(&id, &busy, 103).is_none());
        t.record(
            &record("user", "next", json!({"content":"accepted next"})),
            &id,
            "/owned",
        )
        .unwrap();
        t.snapshot(&id, &idle, 104).unwrap();
        assert_eq!(
            t.snapshot(&id, &busy, 105).unwrap().activity,
            SignalActivity::Working
        );
    }
    #[tokio::test]
    async fn native_registry_reads_only_owned_pid_without_launching_list_processes() {
        let root = tempfile::tempdir().unwrap();
        let sessions = root.path().join("sessions");
        std::fs::create_dir(&sessions).unwrap();
        let row = json!({"pid":123,"cwd":"/owned","sessionId":"native","kind":"interactive","startedAt":1,"status":"busy","version":"2.1.300","peerProtocol":1,"entrypoint":"cli","pidDomain":native_pid_domain().await.unwrap()});
        std::fs::write(sessions.join("123.json"), serde_json::to_vec(&row).unwrap()).unwrap();
        let mut child = row.clone();
        child["sessionId"] = json!("child");
        child["kind"] = json!("background");
        std::fs::write(
            sessions.join("child.json"),
            serde_json::to_vec(&child).unwrap(),
        )
        .unwrap();
        let observer = Observer {
            not_before_ms: 0,
            initial_transcript: Box::default(),
            env: vec![
                ("HOME".into(), root.path().to_string_lossy().into_owned()),
                (
                    "CLAUDE_CONFIG_DIR".into(),
                    root.path().to_string_lossy().into_owned(),
                ),
            ],
        };
        for _ in 0..3 {
            assert_eq!(observer.row("/owned", 123, "native").await.unwrap(), row);
        }
        for field in [
            "pid",
            "cwd",
            "sessionId",
            "kind",
            "startedAt",
            "version",
            "peerProtocol",
            "entrypoint",
            "pidDomain",
        ] {
            let mut bad = row.clone();
            bad[field] = Value::Null;
            std::fs::write(sessions.join("123.json"), serde_json::to_vec(&bad).unwrap()).unwrap();
            assert!(
                observer.row("/owned", 123, "native").await.is_none(),
                "{field}"
            );
        }
        std::fs::write(sessions.join("123.json"), serde_json::to_vec(&row).unwrap()).unwrap();
        let mut oversized = row.clone();
        oversized["padding"] = json!("x".repeat(16 * 1024));
        std::fs::write(
            sessions.join("123.json"),
            serde_json::to_vec(&oversized).unwrap(),
        )
        .unwrap();
        assert!(observer.row("/owned", 123, "native").await.is_none());
        std::fs::write(sessions.join("123.json"), serde_json::to_vec(&row).unwrap()).unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(
            sessions.join("123.json"),
            std::fs::Permissions::from_mode(0o666),
        )
        .unwrap();
        assert!(observer.row("/owned", 123, "native").await.is_none());
        std::fs::set_permissions(
            sessions.join("123.json"),
            std::fs::Permissions::from_mode(0o644),
        )
        .unwrap();
        std::fs::remove_file(sessions.join("123.json")).unwrap();
        std::fs::write(
            sessions.join("child.json"),
            serde_json::to_vec(&row).unwrap(),
        )
        .unwrap();
        std::os::unix::fs::symlink(sessions.join("child.json"), sessions.join("123.json")).unwrap();
        assert!(observer.row("/owned", 123, "native").await.is_none());
    }

    #[tokio::test]
    async fn stale_prior_launch_registry_epoch_cannot_bind_a_reused_owned_pid() {
        let root = tempfile::tempdir().unwrap();
        let env = vec![
            ("HOME".into(), root.path().to_string_lossy().into_owned()),
            (
                "CLAUDE_CONFIG_DIR".into(),
                root.path().to_string_lossy().into_owned(),
            ),
        ];
        let prepared =
            prepare(root.path().to_str().unwrap(), "agent", None, "native", &env).unwrap();
        let Some(NativeObserver::Claude(observer)) = prepared.observer else {
            panic!("wrong observer")
        };
        let path = root.path().join("sessions/123.json");
        std::fs::create_dir(path.parent().unwrap()).unwrap();
        let mut row = json!({"pid":123,"cwd":"/owned","sessionId":"native","kind":"interactive","startedAt":1,"status":"busy","version":"2.1.300","peerProtocol":1,"entrypoint":"cli","pidDomain":native_pid_domain().await.unwrap()});
        std::fs::write(&path, serde_json::to_vec(&row).unwrap()).unwrap();
        assert!(observer.row("/owned", 123, "native").await.is_none());
        row["startedAt"] = json!(now_ms());
        std::fs::write(path, serde_json::to_vec(&row).unwrap()).unwrap();
        assert_eq!(observer.row("/owned", 123, "native").await.unwrap(), row);
    }

    #[test]
    fn out_of_order_native_parent_edges_apply_only_after_current_root_is_proven() {
        let mut transcript = Transcript::default();
        let id = identity();
        transcript
            .record(
                &record("user", "turn", json!({"content":"accepted"})),
                &id,
                "/owned",
            )
            .unwrap();
        let mut child = record("attachment", "child", json!({}));
        child["parentUuid"] = json!("ancestor");
        transcript.record(&child, &id, "/owned").unwrap();
        let mut ask = record(
            "assistant",
            "ask",
            json!({"content":[{"type":"tool_use","id":"request","name":"Write"}]}),
        );
        ask["parentUuid"] = json!("child");
        transcript.record(&ask, &id, "/owned").unwrap();
        let waiting = json!({"status":"waiting","waitingFor":"permission prompt"});
        assert!(transcript.snapshot(&id, &waiting, 100).is_none());
        let mut ancestor = record("attachment", "ancestor", json!({}));
        ancestor["parentUuid"] = json!("turn");
        transcript.record(&ancestor, &id, "/owned").unwrap();
        assert_eq!(
            transcript
                .snapshot(&id, &waiting, 101)
                .unwrap()
                .pending_attention[0]
                .request_id,
            "request"
        );
        let mut result = record(
            "user",
            "answer",
            json!({"content":[{"type":"tool_result","tool_use_id":"request"}]}),
        );
        result["parentUuid"] = json!("ask");
        transcript.record(&result, &id, "/owned").unwrap();
        assert!(transcript.tools.is_empty());
    }

    #[test]
    fn deferred_records_before_a_root_and_retired_branch_facts_remain_separate() {
        let mut t = Transcript::default();
        let id = identity();
        let mut child = record("attachment", "child", json!({}));
        child["parentUuid"] = json!("turn");
        t.record(&child, &id, "/owned").unwrap();
        let mut ask = record(
            "assistant",
            "ask",
            json!({"content":[{"type":"tool_use","id":"request","name":"AskUserQuestion"}]}),
        );
        ask["parentUuid"] = json!("child");
        t.record(&ask, &id, "/owned").unwrap();
        assert!(t.tools.is_empty());
        t.record(
            &record("user", "turn", json!({"content":"accepted"})),
            &id,
            "/owned",
        )
        .unwrap();
        assert_eq!(t.tools.len(), 1);
        let mut old_error = record("assistant", "old-error", json!({"stop_reason":"end_turn"}));
        old_error["isApiErrorMessage"] = json!(true);
        old_error["parentUuid"] = json!("old-meta");
        t.record(&old_error, &id, "/owned").unwrap();
        t.record(
            &record("user", "next", json!({"content":"accepted next"})),
            &id,
            "/owned",
        )
        .unwrap();
        ask["uuid"] = json!("new-ask");
        ask["parentUuid"] = json!("next");
        t.record(&ask, &id, "/owned").unwrap();
        let mut old_meta = record("attachment", "old-meta", json!({}));
        old_meta["parentUuid"] = json!("turn");
        t.record(&old_meta, &id, "/owned").unwrap();
        assert!(t.outcome.is_none());
        assert_eq!(t.tools.len(), 1);
        assert!(!t.pending_records.contains_key("old-error"));
    }

    #[test]
    fn unresolved_parent_history_is_bounded_and_replays_do_not_consume_capacity() {
        let mut t = Transcript::default();
        let id = identity();
        t.record(
            &record("user", "turn", json!({"content":"accepted"})),
            &id,
            "/owned",
        )
        .unwrap();
        for n in 0..4095 {
            let mut edge = record("attachment", &format!("edge-{n}"), json!({}));
            edge["parentUuid"] = json!("missing");
            t.record(&edge, &id, "/owned").unwrap();
        }
        let mut replay = record("attachment", "edge-0", json!({}));
        replay["parentUuid"] = json!("missing");
        t.record(&replay, &id, "/owned").unwrap();
        assert_eq!(t.pending_records.len(), 4095);
        replay["uuid"] = json!("overflow");
        assert!(t.record(&replay, &id, "/owned").is_none());
        assert_eq!(t.pending_records.len(), 4095);
        assert!(t.pending_records.contains_key("edge-0"));
    }

    #[test]
    fn repeated_native_tool_ids_in_new_roots_do_not_reuse_attention_events() {
        let mut t = Transcript::default();
        let id = identity();
        let waiting = json!({"status":"waiting","waitingFor":"input needed"});
        let mut events = Vec::new();
        for root in ["turn", "next"] {
            t.record(
                &record("user", root, json!({"content":"accepted"})),
                &id,
                "/owned",
            )
            .unwrap();
            let mut ask = record(
                "assistant",
                &format!("ask-{root}"),
                json!({"content":[{"type":"tool_use","id":"reused","name":"AskUserQuestion"}],"stop_reason":"tool_use"}),
            );
            ask["parentUuid"] = json!(root);
            t.record(&ask, &id, "/owned").unwrap();
            let pending = t
                .snapshot(&id, &waiting, 100)
                .unwrap()
                .pending_attention
                .remove(0);
            assert_eq!(pending.request_id, "reused");
            assert_eq!(pending.turn_id.as_deref(), Some(root));
            events.push(pending.event_id);
        }
        assert_ne!(events[0], events[1]);
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
        let mut prepared = prepare(
            cwd,
            "probe",
            None,
            context["session"].as_str().unwrap(),
            &env,
        )
        .unwrap();
        // The fixture launches the owned CLI before starting this observer.
        // Use its recorded pre-spawn lower bound, just as production prepare does.
        let Some(NativeObserver::Claude(observer)) = prepared.observer.as_mut() else {
            panic!("wrong observer")
        };
        observer.not_before_ms = context["not_before_ms"]
            .as_i64()
            .expect("recorded fixture launch bound");
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
        let expected = context["expected"].as_str().unwrap();
        let current: Vec<_> = captures
            .iter()
            .filter(|s| s.is_current(s.observation.last_received_at_ms.unwrap()))
            .collect();
        if expected == "reject" {
            assert!(current.iter().all(|s| s.activity != SignalActivity::Working
                && s.outcome.is_none()
                && s.pending_attention.is_empty()));
            return;
        }
        assert!(
            current
                .iter()
                .any(|s| s.activity == SignalActivity::Working)
        );
        if expected == "reject_after_complete" {
            let mut completed = false;
            let mut outcomes = HashSet::new();
            for s in current {
                if let Some(outcome) = &s.outcome {
                    assert_eq!(outcome.kind, OutcomeKind::Completed);
                    outcomes.insert(outcome.event_id.clone());
                    completed = true;
                }
                assert!(!completed || s.activity != SignalActivity::Working);
            }
            assert!(completed);
            assert_eq!(outcomes.len(), 1);
            return;
        }
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
