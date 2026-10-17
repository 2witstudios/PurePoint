//! Passive launch-bound observations. These facts never control a process or answer a request.
use serde::{Deserialize, Serialize};

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
    event_ids: Vec<String>,
    turn_id: Option<String>,
    resolved_request_ids: Vec<String>,
    closed_turn_ids: Vec<String>,
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
            event_ids: Vec::new(),
            turn_id: None,
            resolved_request_ids: Vec::new(),
            closed_turn_ids: Vec::new(),
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
        // Validate the entire payload before committing any replay watermark or state.
        if !valid_event_payload(&event, self.limits) {
            return ApplyResult::Invalid;
        }
        // Known, valid but weak/intent observations consume ordering without renewing facts.
        self.sequence = Some(event.sequence);
        if event.confidence != SignalConfidence::Authoritative {
            return ApplyResult::Ignored;
        }
        if self.is_semantic_noop(&event) {
            return ApplyResult::Ignored;
        }
        let matches_pending = matches!(&event.kind, SignalEventKind::AttentionResolved { request_id }
            if self.snapshot.pending_attention.iter().any(|r| r.request_id == *request_id));
        let recovering = matches!(event.kind, SignalEventKind::Reconciled { .. });
        if matches!(
            self.snapshot.observation.state,
            ObservationState::Disconnected | ObservationState::Lost
        ) && !recovering
        {
            return ApplyResult::Ignored;
        }
        if self.snapshot.observation.state == ObservationState::Current
            && !self.snapshot.is_current(now_ms)
        {
            self.invalidate(ObservationState::Stale);
        }
        let stale_resolution =
            self.snapshot.observation.state == ObservationState::Stale && matches_pending;
        let new_attention_after_stale = self.snapshot.observation.state == ObservationState::Stale
            && self.snapshot.pending_attention.is_empty()
            && matches!(event.kind, SignalEventKind::AttentionPending { .. });
        // Exact removal cannot certify other retained facts. A new request can only
        // recover once stale requests are gone, and must discard stale turn facts.
        let stale_progress = recovering
            || stale_resolution
            || new_attention_after_stale
            || (self.snapshot.pending_attention.is_empty()
                && matches!(
                    event.kind,
                    SignalEventKind::Start | SignalEventKind::Outcome { .. }
                ));
        if self.snapshot.observation.state == ObservationState::Stale && !stale_progress {
            return ApplyResult::Ignored;
        }
        // Replay IDs are safety history, not a rolling cache. Once full, no new
        // authoritative fact may mutate the snapshot without retaining its ID.
        if self.event_ids.len() == self.limits.max_event_ids {
            self.invalidate(ObservationState::Lost);
            return ApplyResult::CapacityExceeded;
        }
        match &event.kind {
            SignalEventKind::Start => {
                if let Some(id) = &self.turn_id
                    && !self.closed_turn_ids.contains(id)
                {
                    if self.closed_turn_ids.len() >= self.limits.max_event_ids {
                        self.invalidate(ObservationState::Lost);
                        return ApplyResult::CapacityExceeded;
                    }
                    self.closed_turn_ids.push(id.clone());
                }
                self.turn_id = event.turn_id.clone();
                self.snapshot.activity = SignalActivity::Working;
                self.snapshot.outcome = None;
            }
            SignalEventKind::Outcome { kind } => {
                if let Some(id) = &event.turn_id {
                    if self.closed_turn_ids.len() >= self.limits.max_event_ids {
                        self.invalidate(ObservationState::Lost);
                        return ApplyResult::CapacityExceeded;
                    }
                    self.closed_turn_ids.push(id.clone());
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
                if self.snapshot.pending_attention.len() >= self.limits.max_pending {
                    self.invalidate(ObservationState::Lost);
                    return ApplyResult::CapacityExceeded;
                }
                if new_attention_after_stale {
                    // This request is current evidence only of itself, not the previous turn.
                    self.snapshot.activity = SignalActivity::Unknown;
                    self.snapshot.outcome = None;
                }
                self.snapshot.pending_attention.push(request.clone());
            }
            SignalEventKind::AttentionResolved { request_id } => {
                if !self.resolved_request_ids.contains(request_id) {
                    if self.resolved_request_ids.len() >= self.limits.max_event_ids {
                        self.invalidate(ObservationState::Lost);
                        return ApplyResult::CapacityExceeded;
                    }
                    self.resolved_request_ids.push(request_id.clone());
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
                let mut to_close = Vec::new();
                if self.turn_id != event.turn_id
                    && let Some(id) = &self.turn_id
                {
                    to_close.push(id.clone());
                }
                if outcome.is_some()
                    && let Some(id) = &event.turn_id
                {
                    to_close.push(id.clone());
                }
                let to_close: Vec<_> = to_close
                    .into_iter()
                    .filter(|id| !self.closed_turn_ids.contains(id))
                    .collect();
                if self.closed_turn_ids.len() + to_close.len() > self.limits.max_event_ids {
                    self.invalidate(ObservationState::Lost);
                    return ApplyResult::CapacityExceeded;
                }
                self.closed_turn_ids.extend(to_close);
                self.turn_id = event.turn_id.clone();
                self.snapshot.activity = *activity;
                self.resolved_request_ids
                    .retain(|id| !pending_attention.iter().any(|r| &r.request_id == id));
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
        self.event_ids.push(event.event_id.clone());
        if matches!(event.kind, SignalEventKind::AttentionResolved { .. })
            && (stale_resolution || !matches_pending)
        {
            // Safety tombstones and precise removals do not renew unrelated state/evidence.
            return ApplyResult::Applied;
        }
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

    fn is_semantic_noop(&self, event: &SignalEvent) -> bool {
        match &event.kind {
            SignalEventKind::Start => event.turn_id.as_ref().is_some_and(|id| {
                self.closed_turn_ids.contains(id) || self.turn_id.as_ref() == Some(id)
            }),
            SignalEventKind::Outcome { .. } => {
                self.snapshot.activity != SignalActivity::Working || event.turn_id != self.turn_id
            }
            SignalEventKind::AttentionPending { request } => {
                self.resolved_request_ids.contains(&request.request_id)
                    || self
                        .snapshot
                        .pending_attention
                        .iter()
                        .any(|r| r.request_id == request.request_id)
            }
            SignalEventKind::AttentionResolved { request_id } => {
                self.resolved_request_ids.contains(request_id)
            }
            SignalEventKind::Reconciled { activity, .. } => {
                (*activity == SignalActivity::Working || self.turn_id != event.turn_id)
                    && event
                        .turn_id
                        .as_ref()
                        .is_some_and(|id| self.closed_turn_ids.contains(id))
            }
            SignalEventKind::StopAttempt
            | SignalEventKind::AttentionIntent { .. }
            | SignalEventKind::RecoverableError => true,
            SignalEventKind::Disconnected => {
                self.snapshot.observation.state == ObservationState::Disconnected
            }
            SignalEventKind::Loss => self.snapshot.observation.state == ObservationState::Lost,
            SignalEventKind::Unknown => false,
        }
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

fn valid_event_payload(event: &SignalEvent, limits: SignalLimits) -> bool {
    match &event.kind {
        SignalEventKind::Unknown => false,
        SignalEventKind::AttentionPending { request } => {
            valid_request(request)
                && request.event_id == event.event_id
                && request.turn_id == event.turn_id
        }
        SignalEventKind::AttentionResolved { request_id } => valid_text(request_id),
        SignalEventKind::AttentionIntent { kind, request_id } => {
            *kind != AttentionKind::Unknown && request_id.as_ref().is_none_or(|s| valid_text(s))
        }
        SignalEventKind::Outcome { kind } => *kind != OutcomeKind::Unknown,
        SignalEventKind::Reconciled {
            activity,
            pending_attention,
            outcome,
        } => {
            pending_attention.len() <= limits.max_pending
                && pending_attention.iter().all(valid_request)
                && !pending_attention.iter().enumerate().any(|(i, r)| {
                    pending_attention[..i]
                        .iter()
                        .any(|p| p.request_id == r.request_id)
                })
                && outcome.as_ref().is_none_or(|o| {
                    o.kind != OutcomeKind::Unknown
                        && valid_text(&o.event_id)
                        && o.confidence == SignalConfidence::Authoritative
                        && o.turn_id == event.turn_id
                        && *activity == SignalActivity::Idle
                })
        }
        _ => true,
    }
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
                max_event_ids: 16,
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
    fn request(sequence: u64, id: &str, kind: AttentionKind) -> SignalEvent {
        event(
            sequence,
            SignalEventKind::AttentionPending {
                request: PendingAttention {
                    request_id: id.into(),
                    kind,
                    event_id: format!("event-{sequence}"),
                    turn_id: Some("turn-1".into()),
                    confidence: SignalConfidence::Authoritative,
                    actionability: AttentionActionability::default(),
                },
            },
        )
    }
    #[test]
    fn given_wrong_launch_session_agent_worktree_or_child_should_leave_root_unchanged() {
        let mut bad = vec![event(1, SignalEventKind::Start); 6];
        bad[0].identity.agent_id = "other".into();
        bad[1].identity.worktree_id = Some("other".into());
        bad[2].identity.generation += 1;
        bad[3].identity.native_session_id = "child".into();
        bad[4].parent_session_id = Some("native-root".into());
        bad[5].identity.launch_id = "other".into();
        for e in bad {
            let mut r = reducer();
            let before = r.snapshot(100);
            assert_eq!(r.apply(e, 100), ApplyResult::Invalid);
            assert_eq!(r.snapshot(100), before);
        }
    }
    #[test]
    fn given_duplicate_or_reordered_events_should_not_refresh_or_replace_terminal_state() {
        let mut r = reducer();
        r.apply(event(2, SignalEventKind::Start), 100);
        r.apply(
            event(
                4,
                SignalEventKind::Outcome {
                    kind: OutcomeKind::Completed,
                },
            ),
            101,
        );
        let before = r.snapshot(102);
        assert_eq!(
            r.apply(event(3, SignalEventKind::Start), 102),
            ApplyResult::Ignored
        );
        assert_eq!(
            r.apply(
                event(
                    4,
                    SignalEventKind::Outcome {
                        kind: OutcomeKind::Errored
                    }
                ),
                102
            ),
            ApplyResult::Ignored
        );
        let mut duplicate = event(5, SignalEventKind::Start);
        duplicate.event_id = "event-2".into();
        assert_eq!(r.apply(duplicate, 102), ApplyResult::Ignored);
        assert_eq!(r.snapshot(102), before);
    }
    #[test]
    fn given_intent_retry_and_non_authoritative_outcome_should_not_finish_or_create_attention() {
        let mut r = reducer();
        r.apply(event(1, SignalEventKind::Start), 100);
        for kind in [
            SignalEventKind::StopAttempt,
            SignalEventKind::RecoverableError,
            SignalEventKind::AttentionIntent {
                kind: AttentionKind::Permission,
                request_id: Some("intent".into()),
            },
        ] {
            r.apply(event(2, kind), 101);
        }
        for confidence in [
            SignalConfidence::Heuristic,
            SignalConfidence::SourceConfirmed,
            SignalConfidence::Unknown,
        ] {
            let mut e = event(
                3,
                SignalEventKind::Outcome {
                    kind: OutcomeKind::Completed,
                },
            );
            e.confidence = confidence;
            r.apply(e, 102);
        }
        let snap = r.snapshot(102);
        assert_eq!(snap.outcome, None);
        assert_eq!(snap.pending_attention, vec![]);
        assert_eq!(snap.observation.last_received_at_ms, Some(100));
        assert_eq!(snap.activity, SignalActivity::Working);
    }
    #[test]
    fn given_multiple_requests_and_background_work_should_resolve_only_matching_id() {
        let mut r = reducer();
        r.apply(event(1, SignalEventKind::Start), 100);
        r.apply(request(2, "permission", AttentionKind::Permission), 101);
        r.apply(request(3, "question", AttentionKind::Question), 102);
        r.apply(
            event(
                4,
                SignalEventKind::AttentionResolved {
                    request_id: "other".into(),
                },
            ),
            103,
        );
        assert_eq!(r.snapshot(103).pending_attention.len(), 2);
        r.apply(
            event(
                5,
                SignalEventKind::AttentionResolved {
                    request_id: "permission".into(),
                },
            ),
            104,
        );
        let snap = r.snapshot(104);
        assert_eq!(snap.activity, SignalActivity::Working);
        assert_eq!(snap.pending_attention.len(), 1);
        assert_eq!(snap.pending_attention[0].request_id, "question");
        assert_eq!(
            snap.pending_attention[0].actionability,
            AttentionActionability::default()
        );
    }
    #[test]
    fn given_each_terminal_outcome_should_preserve_identity_until_confirmed_next_start() {
        for kind in [
            OutcomeKind::Completed,
            OutcomeKind::Errored,
            OutcomeKind::Interrupted,
        ] {
            let mut r = reducer();
            r.apply(event(1, SignalEventKind::Start), 100);
            r.apply(event(2, SignalEventKind::Outcome { kind }), 101);
            let snap = r.snapshot(101);
            assert_eq!(snap.outcome.as_ref().unwrap().kind, kind);
            assert_eq!(snap.outcome.unwrap().event_id, "event-2");
            let mut next = event(3, SignalEventKind::Start);
            next.turn_id = Some("turn-2".into());
            r.apply(next, 102);
            assert_eq!(r.snapshot(102).outcome, None);
            assert_eq!(r.snapshot(102).activity, SignalActivity::Working);
            let mut old = event(4, SignalEventKind::Outcome { kind });
            old.turn_id = Some("turn-1".into());
            assert_eq!(r.apply(old, 103), ApplyResult::Ignored);
            assert_eq!(r.snapshot(103).outcome, None);
        }
    }
    #[test]
    fn given_expired_future_or_invalid_envelope_should_not_change_facts() {
        let mut invalid = vec![event(1, SignalEventKind::Start); 5];
        invalid[0].schema_version = 2;
        invalid[1].occurred_at_ms = 101;
        invalid[2].occurred_at_ms = -900;
        invalid[3].event_id = "".into();
        invalid[4].tool_version = "x".repeat(513);
        for e in invalid {
            let mut r = reducer();
            let before = r.snapshot(100);
            assert_eq!(r.apply(e, 100), ApplyResult::Invalid);
            assert_eq!(r.snapshot(100), before);
        }
    }
    #[test]
    fn given_disconnect_or_loss_should_require_authoritative_full_reconciliation() {
        for kind in [SignalEventKind::Disconnected, SignalEventKind::Loss] {
            let mut r = reducer();
            r.apply(request(1, "pending", AttentionKind::Question), 100);
            r.apply(event(2, kind), 101);
            assert_eq!(
                r.apply(event(3, SignalEventKind::Start), 102),
                ApplyResult::Ignored
            );
            assert_eq!(r.snapshot(102).pending_attention.len(), 1);
            assert!(!r.snapshot(102).is_current(102));
            assert_eq!(
                r.apply(
                    event(
                        4,
                        SignalEventKind::Reconciled {
                            activity: SignalActivity::Idle,
                            pending_attention: vec![],
                            outcome: None
                        }
                    ),
                    103
                ),
                ApplyResult::Applied
            );
            assert!(r.snapshot(103).is_current(103));
            assert_eq!(r.snapshot(103).pending_attention.len(), 0);
        }
    }
    #[test]
    fn given_capacity_exhaustion_should_mark_loss_without_dropping_pending_requests() {
        let mut r = reducer();
        r.apply(request(1, "one", AttentionKind::Question), 100);
        r.apply(request(2, "two", AttentionKind::Question), 101);
        assert_eq!(
            r.apply(request(3, "three", AttentionKind::Permission), 102),
            ApplyResult::CapacityExceeded
        );
        assert_eq!(r.snapshot(102).observation.state, ObservationState::Lost);
        assert_eq!(r.snapshot(102).pending_attention.len(), 2);
    }
    #[test]
    fn given_dead_process_should_never_revive_from_new_telemetry() {
        let mut r = reducer();
        r.invalidate(ObservationState::ProcessDead);
        r.apply(event(1, SignalEventKind::Start), 100);
        r.invalidate(ObservationState::Current);
        assert_eq!(
            r.snapshot(100).observation.state,
            ObservationState::ProcessDead
        );
    }
    #[test]
    fn given_clock_rollback_or_delayed_delivery_should_fail_closed_and_not_extend_sender_lease() {
        let mut r = reducer();
        r.apply(event(1, SignalEventKind::Start), 500);
        assert_eq!(r.snapshot(499).observation.state, ObservationState::Stale);
        assert!(r.snapshot(1099).is_current(1099));
        assert!(!r.snapshot(1100).is_current(1100));
    }
    #[test]
    fn given_resolved_request_id_should_not_resurrect_from_a_replayed_ask() {
        let mut r = reducer();
        r.apply(request(1, "one", AttentionKind::Question), 100);
        r.apply(
            event(
                2,
                SignalEventKind::AttentionResolved {
                    request_id: "one".into(),
                },
            ),
            101,
        );
        assert_eq!(
            r.apply(request(3, "one", AttentionKind::Question), 102),
            ApplyResult::Ignored
        );
        assert_eq!(r.snapshot(102).pending_attention.len(), 0);
    }
    #[test]
    fn given_stale_pending_requests_should_not_revalidate_them_on_unrelated_new_start() {
        let mut r = reducer();
        r.apply(request(1, "old", AttentionKind::Question), 100);
        let mut start = event(2, SignalEventKind::Start);
        start.occurred_at_ms = 1200;
        r.apply(start, 1200);
        assert!(!r.snapshot(1200).is_current(1200));
    }
    #[test]
    fn given_replayed_start_for_closed_turn_should_not_erase_terminal_outcome() {
        let mut r = reducer();
        r.apply(event(1, SignalEventKind::Start), 100);
        r.apply(
            event(
                2,
                SignalEventKind::Outcome {
                    kind: OutcomeKind::Completed,
                },
            ),
            101,
        );
        assert_eq!(
            r.apply(event(3, SignalEventKind::Start), 102),
            ApplyResult::Ignored
        );
        assert_eq!(
            r.snapshot(102).outcome.unwrap().kind,
            OutcomeKind::Completed
        );
    }
    #[test]
    fn given_superseded_active_turn_should_reject_old_start() {
        let mut r = reducer();
        r.apply(event(1, SignalEventKind::Start), 100);
        let mut next = event(2, SignalEventKind::Start);
        next.turn_id = Some("turn-2".into());
        r.apply(next, 101);
        assert_eq!(
            r.apply(event(3, SignalEventKind::Start), 102),
            ApplyResult::Ignored
        );
    }
    #[test]
    fn given_zero_or_oversized_budgets_should_reject_before_allocation() {
        for limits in [
            SignalLimits {
                freshness_ms: 0,
                max_pending: 1,
                max_event_ids: 1,
            },
            SignalLimits {
                freshness_ms: 300001,
                max_pending: 1,
                max_event_ids: 1,
            },
            SignalLimits {
                freshness_ms: 1,
                max_pending: 257,
                max_event_ids: 1,
            },
            SignalLimits {
                freshness_ms: 1,
                max_pending: 1,
                max_event_ids: 4097,
            },
            SignalLimits {
                freshness_ms: 1,
                max_pending: 0,
                max_event_ids: 1,
            },
            SignalLimits {
                freshness_ms: 1,
                max_pending: 1,
                max_event_ids: 0,
            },
        ] {
            assert!(SignalReducer::new(identity(), limits).is_err());
        }
    }
    #[test]
    fn given_replay_safety_capacity_exhaustion_should_require_new_launch_instead_of_eviction() {
        let mut r = SignalReducer::new(
            identity(),
            SignalLimits {
                freshness_ms: 1000,
                max_pending: 2,
                max_event_ids: 4,
            },
        )
        .unwrap();
        for seq in 1..=4 {
            assert_eq!(
                r.apply(
                    event(
                        seq,
                        SignalEventKind::AttentionResolved {
                            request_id: format!("request-{seq}")
                        }
                    ),
                    100
                ),
                ApplyResult::Applied
            );
        }
        assert_eq!(
            r.apply(
                event(
                    5,
                    SignalEventKind::AttentionResolved {
                        request_id: "overflow".into()
                    }
                ),
                100
            ),
            ApplyResult::CapacityExceeded
        );
        assert_eq!(r.snapshot(100).observation.state, ObservationState::Lost);
    }
    #[test]
    fn given_reconciled_new_turn_should_reject_delayed_superseded_start_and_outcome() {
        // given
        let mut r = reducer();
        r.apply(event(1, SignalEventKind::Start), 100);
        let mut reconcile = event(
            2,
            SignalEventKind::Reconciled {
                activity: SignalActivity::Working,
                pending_attention: vec![],
                outcome: None,
            },
        );
        reconcile.turn_id = Some("turn-2".into());
        // when
        assert_eq!(r.apply(reconcile, 101), ApplyResult::Applied);
        let before = r.snapshot(102);
        // then
        assert_eq!(
            r.apply(event(3, SignalEventKind::Start), 102),
            ApplyResult::Ignored
        );
        assert_eq!(
            r.apply(
                event(
                    4,
                    SignalEventKind::Outcome {
                        kind: OutcomeKind::Completed
                    }
                ),
                102
            ),
            ApplyResult::Ignored
        );
        assert_eq!(r.snapshot(102), before);
    }
    #[test]
    fn given_closed_turn_should_reject_reconciliation_reviving_it_as_working() {
        let mut r = reducer();
        r.apply(event(1, SignalEventKind::Start), 100);
        r.apply(
            event(
                2,
                SignalEventKind::Outcome {
                    kind: OutcomeKind::Completed,
                },
            ),
            101,
        );
        let before = r.snapshot(102);
        assert_eq!(
            r.apply(
                event(
                    3,
                    SignalEventKind::Reconciled {
                        activity: SignalActivity::Working,
                        pending_attention: vec![],
                        outcome: None
                    }
                ),
                102
            ),
            ApplyResult::Ignored
        );
        assert_eq!(r.snapshot(102), before);
    }
    #[test]
    fn given_invalid_high_sequence_payload_should_preserve_watermark_for_later_valid_completion() {
        // given
        let mut r = reducer();
        r.apply(event(1, SignalEventKind::Start), 100);
        let before = r.snapshot(101);
        // when
        assert_eq!(
            r.apply(
                event(
                    100,
                    SignalEventKind::AttentionResolved {
                        request_id: String::new()
                    }
                ),
                101
            ),
            ApplyResult::Invalid
        );
        // then
        assert_eq!(r.snapshot(101), before);
        assert_eq!(
            r.apply(
                event(
                    2,
                    SignalEventKind::Outcome {
                        kind: OutcomeKind::Completed
                    }
                ),
                102
            ),
            ApplyResult::Applied
        );
        assert_eq!(
            r.snapshot(102).outcome.unwrap().kind,
            OutcomeKind::Completed
        );
    }
    #[test]
    fn given_closed_superseded_turn_should_reject_terminal_reconciliation_for_it() {
        let mut r = reducer();
        r.apply(event(1, SignalEventKind::Start), 100);
        let mut next = event(2, SignalEventKind::Start);
        next.turn_id = Some("turn-2".into());
        r.apply(next, 101);
        let before = r.snapshot(102);
        let outcome = SignalOutcome {
            kind: OutcomeKind::Completed,
            event_id: "old-terminal".into(),
            turn_id: Some("turn-1".into()),
            confidence: SignalConfidence::Authoritative,
        };
        assert_eq!(
            r.apply(
                event(
                    3,
                    SignalEventKind::Reconciled {
                        activity: SignalActivity::Idle,
                        pending_attention: vec![],
                        outcome: Some(outcome)
                    }
                ),
                102
            ),
            ApplyResult::Ignored
        );
        assert_eq!(r.snapshot(102), before);
    }
    #[test]
    fn given_invalid_pending_or_reconciliation_payload_should_not_consume_sequence() {
        let mut malformed = request(100, "request", AttentionKind::Question);
        if let SignalEventKind::AttentionPending { request } = &mut malformed.kind {
            request.event_id = "mismatch".into();
        }
        let bad_outcome = SignalOutcome {
            kind: OutcomeKind::Completed,
            event_id: "terminal".into(),
            turn_id: Some("other-turn".into()),
            confidence: SignalConfidence::Authoritative,
        };
        let invalid = [
            malformed,
            event(
                100,
                SignalEventKind::Reconciled {
                    activity: SignalActivity::Idle,
                    pending_attention: vec![],
                    outcome: Some(bad_outcome),
                },
            ),
        ];
        for bad in invalid {
            let mut r = reducer();
            r.apply(event(1, SignalEventKind::Start), 100);
            let before = r.snapshot(101);
            assert_eq!(r.apply(bad, 101), ApplyResult::Invalid);
            assert_eq!(r.snapshot(101), before);
            assert_eq!(
                r.apply(
                    event(
                        2,
                        SignalEventKind::Outcome {
                            kind: OutcomeKind::Completed
                        }
                    ),
                    102
                ),
                ApplyResult::Applied
            );
        }
    }
    #[test]
    fn given_reconciliation_exhausting_closed_turn_capacity_should_fail_closed_without_replacing_facts()
     {
        let mut r = SignalReducer::new(
            identity(),
            SignalLimits {
                freshness_ms: 1000,
                max_pending: 2,
                max_event_ids: 1,
            },
        )
        .unwrap();
        r.apply(event(1, SignalEventKind::Start), 100);
        let mut e = event(
            2,
            SignalEventKind::Reconciled {
                activity: SignalActivity::Idle,
                pending_attention: vec![],
                outcome: Some(SignalOutcome {
                    kind: OutcomeKind::Completed,
                    event_id: "end-2".into(),
                    turn_id: Some("turn-2".into()),
                    confidence: SignalConfidence::Authoritative,
                }),
            },
        );
        e.turn_id = Some("turn-2".into());
        assert_eq!(r.apply(e, 101), ApplyResult::CapacityExceeded);
        let snap = r.snapshot(101);
        assert_eq!(snap.observation.state, ObservationState::Lost);
        assert_eq!(snap.activity, SignalActivity::Working);
        assert_eq!(snap.outcome, None);
    }
    #[test]
    fn given_full_event_id_history_should_fail_closed_before_erasing_a_completed_outcome() {
        // given: some hook sources expose no native turn ID, so event IDs are essential.
        let mut r = SignalReducer::new(
            identity(),
            SignalLimits {
                freshness_ms: 1000,
                max_pending: 2,
                max_event_ids: 2,
            },
        )
        .unwrap();
        let mut start = event(1, SignalEventKind::Start);
        start.turn_id = None;
        let mut finish = event(
            2,
            SignalEventKind::Outcome {
                kind: OutcomeKind::Completed,
            },
        );
        finish.turn_id = None;
        assert_eq!(r.apply(start.clone(), 100), ApplyResult::Applied);
        assert_eq!(r.apply(finish, 101), ApplyResult::Applied);
        let before = r.snapshot(102);
        // when: a third distinct event would previously evict the original Start ID.
        let mut resolution = event(
            3,
            SignalEventKind::AttentionResolved {
                request_id: "request".into(),
            },
        );
        resolution.turn_id = None;
        assert_eq!(r.apply(resolution, 102), ApplyResult::CapacityExceeded);
        // then: retained facts stay intact and unavailable, not silently replayable.
        let lost = r.snapshot(102);
        assert_eq!(lost.observation.state, ObservationState::Lost);
        assert_eq!(lost.activity, before.activity);
        assert_eq!(lost.outcome, before.outcome);
        start.sequence = 4;
        assert_eq!(r.apply(start, 103), ApplyResult::Ignored);
        let after = r.snapshot(103);
        assert_eq!(after.outcome, before.outcome);
        assert_eq!(after.activity, SignalActivity::Idle);
        assert_eq!(after.observation.state, ObservationState::Lost);
    }
    #[test]
    fn given_full_event_id_history_should_ignore_duplicates_intent_and_invalid_input_without_losing_freshness()
     {
        let mut r = SignalReducer::new(
            identity(),
            SignalLimits {
                freshness_ms: 1000,
                max_pending: 2,
                max_event_ids: 2,
            },
        )
        .unwrap();
        let mut start = event(1, SignalEventKind::Start);
        start.turn_id = None;
        let mut finish = event(
            2,
            SignalEventKind::Outcome {
                kind: OutcomeKind::Completed,
            },
        );
        finish.turn_id = None;
        r.apply(start.clone(), 100);
        r.apply(finish, 101);
        let before = r.snapshot(102);
        assert_eq!(
            r.apply(event(3, SignalEventKind::StopAttempt), 102),
            ApplyResult::Ignored
        );
        let mut weak = event(4, SignalEventKind::Start);
        weak.confidence = SignalConfidence::Heuristic;
        assert_eq!(r.apply(weak, 102), ApplyResult::Ignored);
        start.sequence = 5;
        assert_eq!(r.apply(start, 102), ApplyResult::Ignored);
        assert_eq!(
            r.apply(
                event(
                    100,
                    SignalEventKind::AttentionResolved {
                        request_id: String::new()
                    }
                ),
                102
            ),
            ApplyResult::Invalid
        );
        assert_eq!(r.snapshot(102), before);
    }
    #[test]
    fn given_exhausted_event_id_history_should_not_regain_current_via_reconciliation() {
        let mut r = SignalReducer::new(
            identity(),
            SignalLimits {
                freshness_ms: 1000,
                max_pending: 2,
                max_event_ids: 1,
            },
        )
        .unwrap();
        r.apply(event(1, SignalEventKind::Start), 100);
        let before = r.snapshot(101);
        let reconciled = event(
            2,
            SignalEventKind::Reconciled {
                activity: SignalActivity::Idle,
                pending_attention: vec![],
                outcome: None,
            },
        );
        assert_eq!(r.apply(reconciled, 101), ApplyResult::CapacityExceeded);
        let retry = event(
            3,
            SignalEventKind::Reconciled {
                activity: SignalActivity::Working,
                pending_attention: vec![],
                outcome: None,
            },
        );
        assert_eq!(r.apply(retry, 102), ApplyResult::CapacityExceeded);
        let after = r.snapshot(102);
        assert_eq!(after.observation.state, ObservationState::Lost);
        assert_eq!(after.activity, before.activity);
        assert_eq!(after.outcome, before.outcome);
        assert_eq!(
            after.observation.last_received_at_ms,
            before.observation.last_received_at_ms
        );
    }
    fn fresh(mut event: SignalEvent, now_ms: i64) -> SignalEvent {
        event.occurred_at_ms = now_ms;
        event
    }
    #[test]
    fn given_late_matching_resolution_should_remove_only_that_request_without_reviving_stale_facts()
    {
        let mut r = reducer();
        r.apply(event(1, SignalEventKind::Start), 100);
        r.apply(request(2, "one", AttentionKind::Question), 101);
        r.apply(request(3, "two", AttentionKind::Permission), 102);
        let before = r.snapshot(1200);
        assert_eq!(before.observation.state, ObservationState::Stale);
        assert_eq!(
            r.apply(
                fresh(
                    event(
                        4,
                        SignalEventKind::AttentionResolved {
                            request_id: "one".into()
                        }
                    ),
                    1200
                ),
                1200
            ),
            ApplyResult::Applied
        );
        let after = r.snapshot(1200);
        assert_eq!(after.pending_attention.len(), 1);
        assert_eq!(after.pending_attention[0].request_id, "two");
        assert_eq!(after.observation, before.observation);
        assert_eq!(after.evidence, before.evidence);
        assert_eq!(after.activity, before.activity);
        assert_eq!(after.outcome, None);
        let mut next = fresh(event(5, SignalEventKind::Start), 1201);
        next.turn_id = Some("next".into());
        assert_eq!(r.apply(next, 1201), ApplyResult::Ignored);
        assert_eq!(
            r.apply(
                fresh(
                    event(
                        6,
                        SignalEventKind::Outcome {
                            kind: OutcomeKind::Completed
                        }
                    ),
                    1202
                ),
                1202
            ),
            ApplyResult::Ignored
        );
        assert_eq!(r.snapshot(1202).pending_attention[0].request_id, "two");
        assert_eq!(r.snapshot(1202).outcome, None);
    }
    #[test]
    fn given_all_stale_requests_resolved_should_accept_later_definitive_outcome_without_inferred_success()
     {
        let mut r = reducer();
        r.apply(event(1, SignalEventKind::Start), 100);
        r.apply(request(2, "one", AttentionKind::Question), 101);
        assert_eq!(
            r.apply(
                fresh(
                    event(
                        3,
                        SignalEventKind::AttentionResolved {
                            request_id: "one".into()
                        }
                    ),
                    1200
                ),
                1200
            ),
            ApplyResult::Applied
        );
        let resolved = r.snapshot(1200);
        assert_eq!(resolved.pending_attention.len(), 0);
        assert_eq!(resolved.observation.state, ObservationState::Stale);
        assert_eq!(resolved.outcome, None);
        assert_eq!(
            r.apply(
                fresh(
                    event(
                        4,
                        SignalEventKind::Outcome {
                            kind: OutcomeKind::Completed
                        }
                    ),
                    1201
                ),
                1201
            ),
            ApplyResult::Applied
        );
        let finished = r.snapshot(1201);
        assert!(finished.is_current(1201));
        assert_eq!(finished.outcome.unwrap().kind, OutcomeKind::Completed);
    }
    #[test]
    fn given_full_history_should_ignore_semantic_repeats_without_losing_current_state() {
        let mut r = SignalReducer::new(
            identity(),
            SignalLimits {
                freshness_ms: 1000,
                max_pending: 2,
                max_event_ids: 2,
            },
        )
        .unwrap();
        r.apply(event(1, SignalEventKind::Start), 100);
        r.apply(
            event(
                2,
                SignalEventKind::AttentionResolved {
                    request_id: "known".into(),
                },
            ),
            101,
        );
        let before = r.snapshot(102);
        assert_eq!(
            r.apply(event(3, SignalEventKind::Start), 102),
            ApplyResult::Ignored
        );
        assert_eq!(
            r.apply(
                event(
                    4,
                    SignalEventKind::AttentionResolved {
                        request_id: "known".into()
                    }
                ),
                102
            ),
            ApplyResult::Ignored
        );
        assert_eq!(r.snapshot(102), before);
        let mut next = event(5, SignalEventKind::Start);
        next.turn_id = Some("next".into());
        assert_eq!(r.apply(next, 103), ApplyResult::CapacityExceeded);
        assert_eq!(r.snapshot(103).observation.state, ObservationState::Lost);
    }
    #[test]
    fn given_unknown_resolution_should_remember_it_once_without_renewing_unrelated_freshness() {
        let mut r = reducer();
        r.apply(event(1, SignalEventKind::Start), 100);
        let before = r.snapshot(101);
        assert_eq!(
            r.apply(
                event(
                    2,
                    SignalEventKind::AttentionResolved {
                        request_id: "unknown".into()
                    }
                ),
                101
            ),
            ApplyResult::Applied
        );
        assert_eq!(r.snapshot(101), before);
        assert_eq!(
            r.apply(
                event(
                    3,
                    SignalEventKind::AttentionResolved {
                        request_id: "unknown".into()
                    }
                ),
                102
            ),
            ApplyResult::Ignored
        );
        assert_eq!(r.snapshot(102), before);
        assert_eq!(
            r.apply(request(4, "unknown", AttentionKind::Question), 103),
            ApplyResult::Ignored
        );
    }
    #[test]
    fn given_pi_style_ui_without_agent_start_should_recover_new_prompt_after_late_answer() {
        let mut r = SignalReducer::new(
            identity(),
            SignalLimits {
                freshness_ms: 30_000,
                max_pending: 4,
                max_event_ids: 16,
            },
        )
        .unwrap();
        let mut first = request(1, "ui-1", AttentionKind::Question);
        first.turn_id = None;
        if let SignalEventKind::AttentionPending { request } = &mut first.kind {
            request.turn_id = None;
        }
        r.apply(first, 100);
        let mut end = fresh(
            event(
                2,
                SignalEventKind::AttentionResolved {
                    request_id: "ui-1".into(),
                },
            ),
            30_101,
        );
        end.turn_id = None;
        assert_eq!(r.apply(end, 30_101), ApplyResult::Applied);
        let after_end = r.snapshot(30_101);
        assert_eq!(after_end.pending_attention.len(), 0);
        assert_eq!(after_end.observation.state, ObservationState::Stale);
        assert_eq!(after_end.outcome, None);
        let mut second = fresh(request(3, "ui-2", AttentionKind::Question), 30_102);
        second.turn_id = None;
        if let SignalEventKind::AttentionPending { request } = &mut second.kind {
            request.turn_id = None;
        }
        assert_eq!(r.apply(second, 30_102), ApplyResult::Applied);
        let next = r.snapshot(30_102);
        assert!(next.is_current(30_102));
        assert_eq!(next.activity, SignalActivity::Unknown);
        assert_eq!(next.outcome, None);
        assert_eq!(next.pending_attention[0].request_id, "ui-2");
        let mut end = fresh(
            event(
                4,
                SignalEventKind::AttentionResolved {
                    request_id: "ui-2".into(),
                },
            ),
            30_103,
        );
        end.turn_id = None;
        assert_eq!(r.apply(end, 30_103), ApplyResult::Applied);
        let done = r.snapshot(30_103);
        assert!(done.is_current(30_103));
        assert!(done.pending_attention.is_empty());
        assert_eq!(done.outcome, None);
        assert_eq!(done.activity, SignalActivity::Unknown);
    }
    #[test]
    fn given_new_attention_after_stale_requests_clear_should_not_revive_old_finished_or_working() {
        for terminal in [false, true] {
            let mut r = reducer();
            r.apply(event(1, SignalEventKind::Start), 100);
            if terminal {
                r.apply(
                    event(
                        2,
                        SignalEventKind::Outcome {
                            kind: OutcomeKind::Completed,
                        },
                    ),
                    101,
                );
            }
            r.apply(request(3, "old", AttentionKind::Question), 102);
            assert_eq!(
                r.apply(
                    fresh(
                        event(
                            4,
                            SignalEventKind::AttentionResolved {
                                request_id: "old".into()
                            }
                        ),
                        1200
                    ),
                    1200
                ),
                ApplyResult::Applied
            );
            assert_eq!(
                r.apply(
                    fresh(request(5, "new", AttentionKind::Question), 1201),
                    1201
                ),
                ApplyResult::Applied
            );
            let after = r.snapshot(1201);
            assert!(after.is_current(1201));
            assert_eq!(after.activity, SignalActivity::Unknown);
            assert_eq!(after.outcome, None);
        }
    }
    #[test]
    fn given_stale_requests_should_reject_wrong_id_replays_and_new_attention_until_exact_removals()
    {
        let mut r = reducer();
        r.apply(event(1, SignalEventKind::Start), 100);
        r.apply(request(2, "one", AttentionKind::Question), 101);
        r.apply(request(3, "two", AttentionKind::Question), 102);
        let before = r.snapshot(1200);
        assert_eq!(
            r.apply(
                fresh(
                    event(
                        4,
                        SignalEventKind::AttentionResolved {
                            request_id: "wrong".into()
                        }
                    ),
                    1200
                ),
                1200
            ),
            ApplyResult::Ignored
        );
        assert_eq!(r.snapshot(1200), before);
        assert_eq!(
            r.apply(
                fresh(
                    event(
                        5,
                        SignalEventKind::AttentionResolved {
                            request_id: "one".into()
                        }
                    ),
                    1201
                ),
                1201
            ),
            ApplyResult::Applied
        );
        let partial = r.snapshot(1201);
        assert_eq!(
            r.apply(
                fresh(
                    event(
                        6,
                        SignalEventKind::AttentionResolved {
                            request_id: "one".into()
                        }
                    ),
                    1202
                ),
                1202
            ),
            ApplyResult::Ignored
        );
        assert_eq!(
            r.apply(
                fresh(request(7, "new-unrelated", AttentionKind::Question), 1203),
                1203
            ),
            ApplyResult::Ignored
        );
        let mut replay = fresh(
            event(
                8,
                SignalEventKind::AttentionResolved {
                    request_id: "two".into(),
                },
            ),
            1204,
        );
        replay.event_id = "event-5".into();
        assert_eq!(r.apply(replay, 1204), ApplyResult::Ignored);
        assert_eq!(r.snapshot(1204), partial);
        assert_eq!(
            r.apply(
                fresh(
                    event(
                        9,
                        SignalEventKind::AttentionResolved {
                            request_id: "two".into()
                        }
                    ),
                    1205
                ),
                1205
            ),
            ApplyResult::Applied
        );
        let mut next = fresh(event(10, SignalEventKind::Start), 1206);
        next.turn_id = Some("next-turn".into());
        assert_eq!(r.apply(next, 1206), ApplyResult::Applied);
        let active = r.snapshot(1206);
        assert!(active.is_current(1206));
        assert_eq!(active.activity, SignalActivity::Working);
        assert_eq!(active.outcome, None);
    }
    #[test]
    fn given_nonstale_health_loss_should_not_use_late_resolution_or_new_prompt_to_restore_current()
    {
        for reason in [
            ObservationState::Lost,
            ObservationState::Disconnected,
            ObservationState::ProcessDead,
        ] {
            let mut r = reducer();
            r.apply(request(1, "pending", AttentionKind::Question), 100);
            r.invalidate(reason);
            let before = r.snapshot(1200);
            assert_eq!(
                r.apply(
                    fresh(
                        event(
                            2,
                            SignalEventKind::AttentionResolved {
                                request_id: "pending".into()
                            }
                        ),
                        1200
                    ),
                    1200
                ),
                ApplyResult::Ignored
            );
            assert_eq!(
                r.apply(
                    fresh(request(3, "new", AttentionKind::Question), 1201),
                    1201
                ),
                ApplyResult::Ignored
            );
            assert_eq!(r.snapshot(1201), before);
        }
    }
    #[test]
    fn given_stale_resolution_at_history_capacity_should_preserve_request_and_fail_closed() {
        let mut r = SignalReducer::new(
            identity(),
            SignalLimits {
                freshness_ms: 1000,
                max_pending: 2,
                max_event_ids: 1,
            },
        )
        .unwrap();
        r.apply(request(1, "pending", AttentionKind::Question), 100);
        assert_eq!(
            r.apply(
                fresh(
                    event(
                        2,
                        SignalEventKind::AttentionResolved {
                            request_id: "pending".into()
                        }
                    ),
                    1200
                ),
                1200
            ),
            ApplyResult::CapacityExceeded
        );
        let snap = r.snapshot(1200);
        assert_eq!(snap.observation.state, ObservationState::Lost);
        assert_eq!(snap.pending_attention[0].request_id, "pending");
        assert_eq!(snap.outcome, None);
    }
    #[test]
    fn given_exact_expiry_or_invalid_late_evidence_should_never_resolve_from_old_child_or_weak_signals()
     {
        let mut r = reducer();
        r.apply(request(1, "pending", AttentionKind::Question), 100);
        let before = r.snapshot(1100);
        for variant in 0..4 {
            let mut e = fresh(
                event(
                    2,
                    SignalEventKind::AttentionResolved {
                        request_id: "pending".into(),
                    },
                ),
                1100,
            );
            match variant {
                0 => e.occurred_at_ms = 100,
                1 => e.parent_session_id = Some("root".into()),
                2 => e.identity.generation += 1,
                _ => e.confidence = SignalConfidence::Heuristic,
            }
            assert_ne!(r.apply(e, 1100), ApplyResult::Applied);
            assert_eq!(r.snapshot(1100), before);
        }
        assert_eq!(
            r.apply(
                fresh(
                    event(
                        3,
                        SignalEventKind::AttentionResolved {
                            request_id: "pending".into()
                        }
                    ),
                    1100
                ),
                1100
            ),
            ApplyResult::Applied
        );
        assert_eq!(r.snapshot(1100).pending_attention.len(), 0);
        assert_eq!(r.snapshot(1100).observation.state, ObservationState::Stale);
    }
    #[test]
    fn given_capacity_should_ignore_duplicate_pending_wrong_outcomes_and_closed_starts() {
        let mut r = SignalReducer::new(
            identity(),
            SignalLimits {
                freshness_ms: 1000,
                max_pending: 2,
                max_event_ids: 2,
            },
        )
        .unwrap();
        r.apply(event(1, SignalEventKind::Start), 100);
        r.apply(request(2, "pending", AttentionKind::Question), 101);
        let before = r.snapshot(102);
        assert_eq!(
            r.apply(request(3, "pending", AttentionKind::Question), 102),
            ApplyResult::Ignored
        );
        let mut wrong = event(
            4,
            SignalEventKind::Outcome {
                kind: OutcomeKind::Completed,
            },
        );
        wrong.turn_id = Some("wrong".into());
        assert_eq!(r.apply(wrong, 102), ApplyResult::Ignored);
        assert_eq!(r.snapshot(102), before);
        let mut r = SignalReducer::new(
            identity(),
            SignalLimits {
                freshness_ms: 1000,
                max_pending: 2,
                max_event_ids: 2,
            },
        )
        .unwrap();
        r.apply(event(1, SignalEventKind::Start), 100);
        r.apply(
            event(
                2,
                SignalEventKind::Outcome {
                    kind: OutcomeKind::Completed,
                },
            ),
            101,
        );
        let before = r.snapshot(102);
        assert_eq!(
            r.apply(event(3, SignalEventKind::Start), 102),
            ApplyResult::Ignored
        );
        assert_eq!(
            r.apply(
                event(
                    4,
                    SignalEventKind::Outcome {
                        kind: OutcomeKind::Errored
                    }
                ),
                102
            ),
            ApplyResult::Ignored
        );
        assert_eq!(r.snapshot(102), before);
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
    #[test]
    fn given_future_signal_enums_and_schema_should_decode_but_fail_closed() {
        let mut value: serde_json::Value =
            serde_json::from_str(include_str!("../tests/fixtures/signals-status-v1.json")).unwrap();
        let v = &mut value["agents"][0]["signal"];
        v["activity"] = "future_activity".into();
        v["pending_attention"][0]["kind"] = "future_attention".into();
        v["pending_attention"][0]["confidence"] = "future_confidence".into();
        v["observation"]["state"] = "future_state".into();
        let snap: SignalSnapshot = serde_json::from_value(v.clone()).unwrap();
        assert_eq!(snap.activity, SignalActivity::Unknown);
        assert_eq!(snap.pending_attention[0].kind, AttentionKind::Unknown);
        assert_eq!(
            snap.pending_attention[0].confidence,
            SignalConfidence::Unknown
        );
        assert!(!snap.is_current(1792195200000));
        v["schema_version"] = 2.into();
        v["observation"]["state"] = "current".into();
        let snap: SignalSnapshot = serde_json::from_value(v.clone()).unwrap();
        assert!(!snap.is_current(1792195200000));
    }
    #[test]
    fn given_old_status_payload_without_signals_should_decode_as_unknown_observation() {
        let mut value: serde_json::Value =
            serde_json::from_str(include_str!("../tests/fixtures/signals-status-v1.json")).unwrap();
        value["agents"][0].as_object_mut().unwrap().remove("signal");
        value["worktrees"][0]["agents"]["ag-worker"]
            .as_object_mut()
            .unwrap()
            .remove("signal");
        let parsed: crate::protocol::Response = serde_json::from_value(value).unwrap();
        let crate::protocol::Response::StatusReport { agents, worktrees } = parsed else {
            panic!("status fixture")
        };
        assert!(agents[0].signal.is_none());
        assert!(worktrees[0].agents["ag-worker"].signal.is_none());
    }
}
