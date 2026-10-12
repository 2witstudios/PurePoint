use serde::{Deserialize, Serialize};

use super::{Error, Result};

macro_rules! string_id {
    ($name:ident) => {
        #[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub String);
        impl $name {
            pub fn new(value: impl Into<String>) -> Result<Self> {
                let value = value.into();
                bounded(&value, 128, stringify!($name))?;
                Ok(Self(value))
            }
        }
    };
}
string_id!(AgentId);
string_id!(RunId);
string_id!(OperationId);

macro_rules! counter {
    ($name:ident) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub u64);
    };
}
counter!(WindowId);
counter!(WriterId);
counter!(ControllerEpoch);
counter!(SessionRevision);

/// SHA-256 (or an equally strong 32-byte digest) of the canonical complete request.
/// The engine computes this; an unauthenticated request's claimed digest is not evidence.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PayloadDigest(pub [u8; 32]);

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum Principal {
    Local { id: String },
    Peer { id: String, generation: u64 },
}

impl Principal {
    pub(super) fn validate(&self) -> Result<()> {
        match self {
            Self::Local { id } => bounded(id, 128, "local principal"),
            Self::Peer { id, generation } => {
                bounded(id, 128, "peer principal")?;
                positive(*generation, "generation")
            }
        }
    }

    pub(super) fn same_identity(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Local { id: a }, Self::Local { id: b })
            | (Self::Peer { id: a, .. }, Self::Peer { id: b, .. }) => a == b,
            _ => false,
        }
    }
}

/// Trusted ingress context, deliberately not deserializable from a wire request.
/// Constructors attest authentication only; the engine must separately authorize grants.
#[derive(Clone, Debug)]
pub struct Caller {
    principal: Principal,
}

impl Caller {
    pub fn authenticated_local(id: impl Into<String>) -> Result<Self> {
        let principal = Principal::Local { id: id.into() };
        principal.validate()?;
        Ok(Self { principal })
    }

    pub fn authenticated_peer(id: impl Into<String>, generation: u64) -> Result<Self> {
        let principal = Principal::Peer {
            id: id.into(),
            generation,
        };
        principal.validate()?;
        Ok(Self { principal })
    }

    pub fn principal(&self) -> &Principal {
        &self.principal
    }
}

/// Separate local-policy capability. The engine creates it only after explicit local
/// administrative authorization, never from a peer-supplied flag. Not deserializable.
#[derive(Clone, Debug)]
pub struct LocalAdmin {
    pub(super) caller: Caller,
}

impl LocalAdmin {
    pub fn authorized_by_local_policy(caller: &Caller) -> Result<Self> {
        if !matches!(caller.principal(), Principal::Local { .. }) {
            return Err(Error::Unauthorized);
        }
        Ok(Self {
            caller: caller.clone(),
        })
    }
}

#[derive(Clone, Debug)]
pub struct Limits {
    pub global_runs: usize,
    pub project_runs: usize,
    pub runtime_runs: usize,
    pub principal_runs: usize,
    pub starting_runs: usize,
    pub windows: usize,
    pub operations: usize,
    pub agents: usize,
    pub runs: usize,
    pub peers: usize,
    pub writers: usize,
    pub input_receipts: usize,
    pub receipts_per_writer: usize,
    pub max_window_seconds: i64,
    pub max_input_bytes: u32,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            global_runs: 256,
            project_runs: 64,
            runtime_runs: 128,
            principal_runs: 64,
            starting_runs: 16,
            windows: 1024,
            operations: 4096,
            agents: 1024,
            runs: 4096,
            peers: 256,
            writers: 4096,
            input_receipts: 8192,
            receipts_per_writer: 64,
            max_window_seconds: 86_400,
            max_input_bytes: 65_536,
        }
    }
}

impl Limits {
    pub(super) fn validate(&self) -> Result<()> {
        for limit in [
            self.global_runs,
            self.project_runs,
            self.runtime_runs,
            self.principal_runs,
            self.starting_runs,
            self.windows,
            self.operations,
            self.agents,
            self.runs,
            self.peers,
            self.writers,
            self.input_receipts,
            self.receipts_per_writer,
        ] {
            if limit > 100_000 {
                return Err(Error::Invalid("limit exceeds hard record bound"));
            }
        }
        if self.max_window_seconds <= 0
            || self.max_window_seconds > 86_400 * 30
            || self.max_input_bytes == 0
            || self.max_input_bytes > 1_048_576
        {
            return Err(Error::Invalid("window or input limit"));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperationKey {
    pub window: WindowId,
    pub id: OperationId,
    pub digest: PayloadDigest,
}

impl OperationKey {
    pub(super) fn validate(&self) -> Result<()> {
        positive(self.window.0, "window")?;
        bounded(&self.id.0, 128, "operation ID")
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperationWindow {
    pub id: WindowId,
    pub principal: Principal,
    pub expires_at: i64,
    pub closed: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorktreeIntent {
    pub id: String,
    pub path: String,
    pub branch: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LaunchIntent {
    /// Canonical worker-local project identity, resolved and authorized by the engine.
    pub project: String,
    pub export: Option<String>,
    pub runtime: String,
    pub source_commit: String,
    pub worktree: WorktreeIntent,
}

impl LaunchIntent {
    pub(super) fn validate(&self) -> Result<()> {
        bounded(&self.project, 4096, "project")?;
        if let Some(export) = &self.export {
            bounded(export, 128, "export")?;
        }
        bounded(&self.runtime, 128, "runtime")?;
        if !matches!(self.source_commit.len(), 40 | 64)
            || !self
                .source_commit
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(Error::Invalid("exact source commit required"));
        }
        bounded(&self.worktree.id, 128, "worktree ID")?;
        bounded(&self.worktree.path, 4096, "worktree path")?;
        bounded(&self.worktree.branch, 256, "worktree branch")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Lifecycle {
    Accepted,
    Starting,
    Running,
    Stopping,
    Unknown,
    Failed,
    Terminated,
    Interrupted,
}

impl Lifecycle {
    pub fn holds_capacity(self) -> bool {
        matches!(
            self,
            Self::Accepted | Self::Starting | Self::Running | Self::Stopping | Self::Unknown
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MutationGuard {
    pub agent_id: AgentId,
    pub run_id: RunId,
    pub epoch: ControllerEpoch,
    pub revision: SessionRevision,
}

impl MutationGuard {
    pub(super) fn validate(&self) -> Result<()> {
        bounded(&self.agent_id.0, 128, "agent ID")?;
        bounded(&self.run_id.0, 128, "run ID")?;
        positive(self.epoch.0, "epoch")?;
        positive(self.revision.0, "revision")
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentRecord {
    pub agent_id: AgentId,
    pub owner_daemon: String,
    pub controller: Option<Principal>,
    pub epoch: ControllerEpoch,
    pub revision: SessionRevision,
    pub current_run: RunId,
    pub lifecycle: Lifecycle,
    /// Last writer remains referenced even when fenced, to require explicit reconciliation.
    pub current_writer: Option<WriterId>,
}

impl AgentRecord {
    pub fn guard(&self) -> MutationGuard {
        MutationGuard {
            agent_id: self.agent_id.clone(),
            run_id: self.current_run.clone(),
            epoch: self.epoch,
            revision: self.revision,
        }
    }
}

/// An engine-owned supervisor identity (e.g. guardian instance + run token).
/// A bare PID is never sufficient; this module does not probe, adopt, or kill processes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SupervisionIdentity {
    pub mechanism: String,
    pub token: String,
}

impl SupervisionIdentity {
    pub(super) fn validate(&self) -> Result<()> {
        bounded(&self.mechanism, 128, "supervision mechanism")?;
        bounded(&self.token, 1024, "supervision token")
    }
}

/// Trusted supervisor evidence, not a remotely asserted status. Evidence must identify
/// this exact run and establish the entire supported process tree's outcome.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum OutcomeProof {
    PrelaunchFailure { evidence: String },
    ProcessTreeTerminated { evidence: String, interrupted: bool },
}

impl OutcomeProof {
    pub(super) fn validate(&self) -> Result<()> {
        let (Self::PrelaunchFailure { evidence } | Self::ProcessTreeTerminated { evidence, .. }) =
            self;
        bounded(evidence, 1024, "outcome evidence")
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunRecord {
    pub run_id: RunId,
    pub agent_id: AgentId,
    pub principal: Principal,
    pub intent: LaunchIntent,
    pub lifecycle: Lifecycle,
    /// Unknown startup continues to consume a startup slot until explicit reconciliation.
    pub startup_pending: bool,
    pub supervision: Option<SupervisionIdentity>,
    pub outcome: Option<OutcomeProof>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LaunchReceipt {
    pub agent_id: AgentId,
    pub run_id: RunId,
    pub guard: MutationGuard,
}

/// Only a fresh durable acceptance permits one attempt. A replayed receipt is read-only
/// evidence, even when its originally stored lifecycle says Accepted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Recorded<T> {
    pub value: T,
    pub fresh: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum OperationReceipt {
    Launch(LaunchReceipt),
    Agent(AgentRecord),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PeerRecord {
    pub id: String,
    pub generation: u64,
    pub revoked: bool,
    /// Increments only on explicit policy anchoring, independently of restart
    /// and host-control generations. Registration starts at zero (deny).
    pub authorization_revision: u64,
    /// The engine must compare durable canonical policy bytes to this anchor
    /// before every dispatch under its authority barrier. None denies peer use.
    pub authorization_digest: Option<PayloadDigest>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WriterCheckpoint {
    pub writer: WriterId,
    pub last_sequence: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WriterRecord {
    pub id: WriterId,
    pub agent_id: AgentId,
    pub run_id: RunId,
    pub epoch: ControllerEpoch,
    pub principal: Principal,
    pub last_sequence: u64,
    pub live: bool,
}

impl WriterRecord {
    pub fn checkpoint(&self) -> WriterCheckpoint {
        WriterCheckpoint {
            writer: self.id,
            last_sequence: self.last_sequence,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum InputOutcome {
    Accepted,
    Written,
    Partial { bytes: u32 },
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InputAck {
    pub writer: WriterId,
    pub sequence: u64,
    pub digest: PayloadDigest,
    pub payload_bytes: u32,
    pub outcome: InputOutcome,
}

pub(super) fn bounded(value: &str, max: usize, name: &'static str) -> Result<()> {
    if value.is_empty() || value.len() > max || value.chars().any(char::is_control) {
        return Err(Error::Invalid(name));
    }
    Ok(())
}

pub(super) fn positive(value: u64, name: &'static str) -> Result<()> {
    if value == 0 || value > i64::MAX as u64 {
        return Err(Error::Invalid(name));
    }
    Ok(())
}

pub(super) fn increment(value: u64) -> Result<u64> {
    let next = value.checked_add(1).ok_or(Error::CounterExhausted)?;
    if next > i64::MAX as u64 {
        return Err(Error::CounterExhausted);
    }
    Ok(next)
}
