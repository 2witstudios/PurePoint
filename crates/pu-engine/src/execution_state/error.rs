#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CapacityScope {
    Global,
    Project,
    Runtime,
    Principal,
    Starting,
    /// Every principal's live records together.
    Records(&'static str),
    /// All peers' live records together; excludes the local owner reserve.
    PeerRecords(&'static str),
    /// One peer principal's own live records.
    PrincipalRecords(&'static str),
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("invalid {0}")]
    Invalid(&'static str),
    #[error("unauthorized caller")]
    Unauthorized,
    #[error("peer is missing, unanchored, revoked, or has a stale generation")]
    PeerFenced,
    #[error("unknown or expired operation window")]
    WindowClosed,
    #[error("operation or sequence payload conflict")]
    PayloadConflict,
    #[error("capacity exhausted: {scope:?} (limit {limit})")]
    Capacity { scope: CapacityScope, limit: usize },
    #[error("record not found: {0}")]
    NotFound(&'static str),
    #[error("stale agent guard; current state: {0:?}")]
    Stale(Box<super::AgentRecord>),
    #[error("controller does not authorize this mutation")]
    ControllerFenced,
    #[error("invalid lifecycle transition")]
    InvalidTransition,
    #[error("old run still holds capacity")]
    RunUnresolved,
    #[error("writer is fenced or unknown")]
    WriterFenced,
    #[error("writer reconciliation checkpoint required")]
    ReconciliationRequired,
    #[error("expected next input sequence {expected}")]
    SequenceGap { expected: u64 },
    #[error("input acknowledgement was compacted")]
    ReceiptExpired,
    #[error("writer has outstanding accepted input")]
    InputPending,
    #[error("counter exhausted")]
    CounterExhausted,
    #[error("unknown or corrupt execution ledger schema")]
    Schema,
    #[error("corrupt execution ledger record: {0}")]
    Corrupt(&'static str),
    #[error("unsafe execution ledger path: {0}")]
    UnsafePath(&'static str),
    #[error("execution ledger mutex poisoned")]
    Poisoned,
    #[error("SQLite execution ledger error: {0}")]
    Storage(#[from] rusqlite::Error),
    #[error("execution ledger filesystem error: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, Error>;
