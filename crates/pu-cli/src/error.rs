use thiserror::Error;

#[derive(Debug, Error)]
pub enum CliError {
    #[error("daemon not running — run `pu spawn` to start it")]
    DaemonNotRunning,

    #[error("daemon request timed out after 30 seconds")]
    RequestTimeout,

    #[error("daemon returned error [{code}]: {message}")]
    DaemonError { code: String, message: String },

    #[error("{0}")]
    Io(#[from] std::io::Error),

    #[error("{0}")]
    Json(#[from] serde_json::Error),

    #[error("{0}")]
    Other(String),
}
