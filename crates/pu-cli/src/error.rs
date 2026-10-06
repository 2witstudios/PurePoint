use thiserror::Error;

#[derive(Debug, Error)]
pub enum CliError {
    #[error("daemon not running — it should auto-start; check `~/.pu/logs/daemon.log` for errors")]
    DaemonNotRunning,

    #[error("daemon request timed out after {0} seconds")]
    RequestTimeout(u64),

    #[error("daemon returned error [{code}]: {message}")]
    DaemonError { code: String, message: String },

    #[error("{0}")]
    Io(#[from] std::io::Error),

    #[error("{0}")]
    Json(#[from] serde_json::Error),

    #[error("{0}")]
    Other(String),
}
