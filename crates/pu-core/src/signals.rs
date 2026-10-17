//! Passive launch-bound observations. These facts never control a process or answer a request.
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

pub const SIGNAL_SCHEMA_VERSION: u32 = 1;

macro_rules! signal_enum {
    ($name:ident { $($variant:ident),* $(,)? }) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
        #[serde(rename_all = "snake_case")]
        pub enum $name { $($variant,)* #[serde(other)] Unknown }
    };
}
signal_enum!(SignalConfidence {
    Authoritative,
    SourceConfirmed,
    Heuristic
});
signal_enum!(SignalActivity { Working, Idle });
signal_enum!(AttentionKind {
    Permission,
    Question
});
signal_enum!(OutcomeKind {
    Completed,
    Errored,
    Interrupted
});
signal_enum!(ObservationState {
    Current,
    Unavailable,
    Stale,
    Disconnected,
    Lost,
    ProcessDead
});

/// Bound by the owner at launch; cwd is deliberately absent as an identity key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignalIdentity {
    pub agent_id: String,
    pub worktree_id: Option<String>,
    pub launch_id: String,
    pub generation: u64,
    pub native_session_id: String,
}

/// True only after exact-request discovery/response/resolution is qualified on that platform.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttentionActionability {
    #[serde(default)]
    pub macos: bool,
    #[serde(default)]
    pub ios: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingAttention {
    pub request_id: String,
    pub kind: AttentionKind,
    pub event_id: String,
    pub turn_id: Option<String>,
    pub confidence: SignalConfidence,
    #[serde(default)]
    pub actionability: AttentionActionability,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignalOutcome {
    pub kind: OutcomeKind,
    pub event_id: String,
    pub turn_id: Option<String>,
    pub confidence: SignalConfidence,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignalEvidence {
    pub tool: String,
    pub tool_version: String,
    pub adapter_version: String,
    pub confidence: SignalConfidence,
    pub event_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignalObservation {
    pub state: ObservationState,
    pub last_received_at_ms: Option<i64>,
    pub expires_at_ms: Option<i64>,
}

/// Retained facts are diagnostic when observation is not current; clients must gate them first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignalSnapshot {
    pub schema_version: u32,
    pub identity: SignalIdentity,
    pub activity: SignalActivity,
    pub pending_attention: Vec<PendingAttention>,
    pub outcome: Option<SignalOutcome>,
    pub observation: SignalObservation,
    pub evidence: Option<SignalEvidence>,
}

impl SignalSnapshot {
    /// Both bounds must hold; clock rollback and absent expiry fail closed.
    pub fn is_current(&self, now_ms: i64) -> bool {
        self.schema_version == SIGNAL_SCHEMA_VERSION
            && self.observation.state == ObservationState::Current
            && matches!((self.observation.last_received_at_ms, self.observation.expires_at_ms),
                (Some(start), Some(end)) if start <= now_ms && now_ms < end)
    }
}

/// Normalized facts, not raw native payloads. Adapters must classify intent conservatively.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SignalEventKind {
    Start,
    StopAttempt,
    Outcome {
        kind: OutcomeKind,
    },
    AttentionIntent {
        kind: AttentionKind,
        request_id: Option<String>,
    },
    AttentionPending {
        request: PendingAttention,
    },
    AttentionResolved {
        request_id: String,
    },
    RecoverableError,
    Disconnected,
    Loss,
    /// Full authoritative root snapshot after loss/reconnect, never a heartbeat.
    Reconciled {
        activity: SignalActivity,
        pending_attention: Vec<PendingAttention>,
        outcome: Option<SignalOutcome>,
    },
    #[serde(other)]
    Unknown,
}

/// Sequence is strictly increasing per launch and assigned by the adapter's ordered intake.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignalEvent {
    pub schema_version: u32,
    pub tool: String,
    pub tool_version: String,
    pub adapter_version: String,
    pub identity: SignalIdentity,
    pub parent_session_id: Option<String>,
    pub turn_id: Option<String>,
    pub event_id: String,
    pub sequence: u64,
    pub occurred_at_ms: i64,
    pub confidence: SignalConfidence,
    pub kind: SignalEventKind,
}

/// Explicit finite memory/time budgets. No silently unbounded defaults.
#[derive(Debug, Clone, Copy)]
pub struct SignalLimits {
    pub freshness_ms: u32,
    pub max_pending: usize,
    pub max_event_ids: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApplyResult {
    Applied,
    Ignored,
    Invalid,
    CapacityExceeded,
}

/// Pure reducer: the caller supplies time and owns all transport and launch lifecycle.
pub struct SignalReducer {
    snapshot: SignalSnapshot,
    limits: SignalLimits,
    sequence: Option<u64>,
    event_ids: VecDeque<String>,
    turn_id: Option<String>,
}

impl SignalReducer {
    /// Reject invalid identities and oversized budgets before allocating state.
    pub fn new(identity: SignalIdentity, limits: SignalLimits) -> Result<Self, &'static str> {
        if !valid_identity(&identity)
            || !(1..=300_000).contains(&limits.freshness_ms)
            || !(1..=256).contains(&limits.max_pending)
            || !(1..=4096).contains(&limits.max_event_ids)
        {
            return Err("invalid signal identity or limits");
        }
        Ok(Self {
            snapshot: SignalSnapshot {
                schema_version: SIGNAL_SCHEMA_VERSION,
                identity,
                activity: SignalActivity::Unknown,
                pending_attention: Vec::new(),
                outcome: None,
                observation: SignalObservation {
                    state: ObservationState::Unavailable,
                    last_received_at_ms: None,
                    expires_at_ms: None,
                },
                evidence: None,
            },
            limits,
            sequence: None,
            event_ids: VecDeque::new(),
            turn_id: None,
        })
    }

    /// Apply an ordered root observation. Invalid/replayed input never refreshes freshness.
    pub fn apply(&mut self, event: SignalEvent, now_ms: i64) -> ApplyResult {
        if event.schema_version != SIGNAL_SCHEMA_VERSION
            || event.identity != self.snapshot.identity
            || event.parent_session_id.is_some()
            || !valid_text(&event.event_id)
            || !valid_text(&event.tool)
            || !valid_text(&event.tool_version)
            || !valid_text(&event.adapter_version)
            || event.turn_id.as_ref().is_some_and(|s| !valid_text(s))
            || event.occurred_at_ms > now_ms
            || now_ms.saturating_sub(event.occurred_at_ms) >= i64::from(self.limits.freshness_ms)
        {
            return ApplyResult::Invalid;
        }
        if self.sequence.is_some_and(|s| event.sequence <= s)
            || self.event_ids.contains(&event.event_id)
        {
            return ApplyResult::Ignored;
        }
        if self.snapshot.observation.state == ObservationState::ProcessDead {
            return ApplyResult::Ignored;
        }
        if matches!(event.kind, SignalEventKind::Unknown) {
            return ApplyResult::Invalid;
        }
        if event.confidence != SignalConfidence::Authoritative {
            return ApplyResult::Ignored;
        }
        let recovering = matches!(event.kind, SignalEventKind::Reconciled { .. });
        if matches!(
            self.snapshot.observation.state,
            ObservationState::Disconnected | ObservationState::Lost
        ) && !recovering
        {
            return ApplyResult::Ignored;
        }
        match &event.kind {
            SignalEventKind::Start => {
                self.turn_id = event.turn_id.clone();
                self.snapshot.activity = SignalActivity::Working;
                self.snapshot.outcome = None;
            }
            SignalEventKind::Outcome { kind } => {
                if *kind == OutcomeKind::Unknown
                    || self.snapshot.activity != SignalActivity::Working
                    || event.turn_id != self.turn_id
                {
                    return ApplyResult::Ignored;
                }
                self.snapshot.activity = SignalActivity::Idle;
                self.snapshot.outcome = Some(SignalOutcome {
                    kind: *kind,
                    event_id: event.event_id.clone(),
                    turn_id: event.turn_id.clone(),
                    confidence: event.confidence,
                });
            }
            SignalEventKind::AttentionPending { request } => {
                if !valid_request(request)
                    || request.event_id != event.event_id
                    || request.turn_id != event.turn_id
                {
                    return ApplyResult::Invalid;
                }
                if self
                    .snapshot
                    .pending_attention
                    .iter()
                    .any(|r| r.request_id == request.request_id)
                {
                    return ApplyResult::Ignored;
                }
                if self.snapshot.pending_attention.len() >= self.limits.max_pending {
                    self.invalidate(ObservationState::Lost);
                    return ApplyResult::CapacityExceeded;
                }
                self.snapshot.pending_attention.push(request.clone());
            }
            SignalEventKind::AttentionResolved { request_id } => {
                if !valid_text(request_id) {
                    return ApplyResult::Invalid;
                }
                self.snapshot
                    .pending_attention
                    .retain(|r| r.request_id != *request_id);
            }
            SignalEventKind::Disconnected => self.invalidate(ObservationState::Disconnected),
            SignalEventKind::Loss => self.invalidate(ObservationState::Lost),
            SignalEventKind::Reconciled {
                activity,
                pending_attention,
                outcome,
            } => {
                if pending_attention.len() > self.limits.max_pending
                    || pending_attention.iter().any(|r| !valid_request(r))
                    || pending_attention.iter().enumerate().any(|(i, r)| {
                        pending_attention[..i]
                            .iter()
                            .any(|p| p.request_id == r.request_id)
                    })
                    || outcome.as_ref().is_some_and(|o| {
                        o.kind == OutcomeKind::Unknown
                            || !valid_text(&o.event_id)
                            || o.confidence != SignalConfidence::Authoritative
                            || o.turn_id != event.turn_id
                    })
                    || (*activity == SignalActivity::Working && outcome.is_some())
                {
                    return ApplyResult::Invalid;
                }
                self.turn_id = event.turn_id.clone();
                self.snapshot.activity = *activity;
                self.snapshot.pending_attention = pending_attention.clone();
                self.snapshot.outcome = outcome.clone();
                self.snapshot.observation.state = ObservationState::Current;
            }
            // Intent, attempted stop and recoverable failure do not establish or refresh facts.
            SignalEventKind::StopAttempt
            | SignalEventKind::AttentionIntent { .. }
            | SignalEventKind::RecoverableError => return ApplyResult::Ignored,
            SignalEventKind::Unknown => return ApplyResult::Invalid,
        }
        self.sequence = Some(event.sequence);
        if self.event_ids.len() == self.limits.max_event_ids {
            self.event_ids.pop_front();
        }
        self.event_ids.push_back(event.event_id.clone());
        if !matches!(
            event.kind,
            SignalEventKind::Disconnected | SignalEventKind::Loss
        ) {
            self.snapshot.observation.state = ObservationState::Current;
            self.snapshot.observation.last_received_at_ms = Some(now_ms);
            // Sender age counts against freshness; delayed telemetry gets no fresh lease.
            self.snapshot.observation.expires_at_ms = Some(
                event
                    .occurred_at_ms
                    .saturating_add(i64::from(self.limits.freshness_ms)),
            );
        }
        self.snapshot.evidence = Some(SignalEvidence {
            tool: event.tool,
            tool_version: event.tool_version,
            adapter_version: event.adapter_version,
            confidence: event.confidence,
            event_id: event.event_id,
        });
        ApplyResult::Applied
    }

    /// Expiry never infers successful completion or resolves pending requests.
    pub fn snapshot(&self, now_ms: i64) -> SignalSnapshot {
        let mut snapshot = self.snapshot.clone();
        if snapshot.observation.state == ObservationState::Current && !snapshot.is_current(now_ms) {
            snapshot.observation.state = ObservationState::Stale;
        }
        snapshot
    }

    /// Invalidation cannot restore freshness; process death is irreversible for this launch.
    pub fn invalidate(&mut self, reason: ObservationState) {
        if self.snapshot.observation.state == ObservationState::ProcessDead {
            return;
        }
        if matches!(
            reason,
            ObservationState::Unavailable
                | ObservationState::Stale
                | ObservationState::Disconnected
                | ObservationState::Lost
                | ObservationState::ProcessDead
                | ObservationState::Unknown
        ) {
            self.snapshot.observation.state = reason;
        }
    }
}

fn valid_text(s: &str) -> bool {
    !s.trim().is_empty() && s.len() <= 512
}
fn valid_identity(i: &SignalIdentity) -> bool {
    valid_text(&i.agent_id)
        && valid_text(&i.launch_id)
        && valid_text(&i.native_session_id)
        && i.worktree_id.as_ref().is_none_or(|s| valid_text(s))
}
fn valid_request(r: &PendingAttention) -> bool {
    valid_text(&r.request_id)
        && valid_text(&r.event_id)
        && r.kind != AttentionKind::Unknown
        && r.confidence == SignalConfidence::Authoritative
        && r.turn_id.as_ref().is_none_or(|s| valid_text(s))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn identity() -> SignalIdentity {
        SignalIdentity {
            agent_id: "ag-root".into(),
            worktree_id: None,
            launch_id: "launch-1".into(),
            generation: 1,
            native_session_id: "native-root".into(),
        }
    }
    fn reducer() -> SignalReducer {
        SignalReducer::new(
            identity(),
            SignalLimits {
                freshness_ms: 1000,
                max_pending: 2,
                max_event_ids: 4,
            },
        )
        .unwrap()
    }
    fn event(sequence: u64, kind: SignalEventKind) -> SignalEvent {
        SignalEvent {
            schema_version: 1,
            tool: "fixture".into(),
            tool_version: "1.0".into(),
            adapter_version: "1".into(),
            identity: identity(),
            parent_session_id: None,
            turn_id: Some("turn-1".into()),
            event_id: format!("event-{sequence}"),
            sequence,
            occurred_at_ms: 100,
            confidence: SignalConfidence::Authoritative,
            kind,
        }
    }
    #[test]
    fn given_start_then_stop_attempt_should_not_invent_completion() {
        // given
        let mut r = reducer();
        // when
        assert_eq!(
            r.apply(event(1, SignalEventKind::Start), 100),
            ApplyResult::Applied
        );
        r.apply(event(2, SignalEventKind::StopAttempt), 101);
        // then
        assert_eq!(r.snapshot(101).activity, SignalActivity::Working);
        assert_eq!(r.snapshot(101).outcome, None);
    }
    #[test]
    fn given_silence_or_process_death_should_invalidate_without_success() {
        let mut r = reducer();
        r.apply(event(1, SignalEventKind::Start), 100);
        assert_eq!(r.snapshot(1100).observation.state, ObservationState::Stale);
        r.invalidate(ObservationState::ProcessDead);
        assert_eq!(
            r.snapshot(101).observation.state,
            ObservationState::ProcessDead
        );
        assert_eq!(r.snapshot(101).outcome, None);
    }
}

#[cfg(test)]
mod wire_tests {
    use super::*;
    #[test]
    fn given_status_fixture_should_preserve_root_and_worktree_signal_facts() {
        let wire = include_str!("../tests/fixtures/signals-status-v1.json");
        let parsed: crate::protocol::Response = serde_json::from_str(wire).unwrap();
        let crate::protocol::Response::StatusReport { agents, worktrees } = parsed else {
            panic!("status fixture");
        };
        assert_eq!(
            agents[0].signal.as_ref().unwrap().pending_attention[0].request_id,
            "request-1"
        );
        assert_eq!(
            worktrees[0].agents["ag-worker"]
                .signal
                .as_ref()
                .unwrap()
                .outcome
                .as_ref()
                .unwrap()
                .kind,
            OutcomeKind::Completed
        );
        assert_eq!(
            serde_json::to_value(worktrees[0].clone()).unwrap()["agents"]["ag-worker"]["signal"]["identity"]
                ["generation"],
            1
        );
    }
    #[test]
    fn given_inventory_fixture_should_preserve_signal_and_old_payload_compatibility() {
        let mut value: serde_json::Value =
            serde_json::from_str(include_str!("../tests/fixtures/signals-inventory-v1.json"))
                .unwrap();
        let parsed: crate::protocol::InventoryReport =
            serde_json::from_value(value.clone()).unwrap();
        assert_eq!(
            parsed.agents[0].signal.as_ref().unwrap().identity.agent_id,
            "ag-root"
        );
        value["agents"][0].as_object_mut().unwrap().remove("signal");
        let parsed: crate::protocol::InventoryReport = serde_json::from_value(value).unwrap();
        assert_eq!(parsed.agents[0].signal, None);
    }
}
