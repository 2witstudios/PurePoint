use thiserror::Error;

#[derive(Debug, Error)]
pub enum PuError {
    #[error("not initialized — run `pu init` first")]
    NotInitialized,

    #[error("already initialized at {0}")]
    AlreadyInitialized(String),

    #[error("agent not found: {0}")]
    AgentNotFound(String),

    #[error("worktree not found: {0}")]
    WorktreeNotFound(String),

    #[error("daemon not running")]
    DaemonNotRunning,

    #[error("daemon connection failed: {0}")]
    DaemonConnectionFailed(String),

    #[error("protocol version mismatch (client={client}, daemon={daemon}) — restart the daemon")]
    ProtocolMismatch { client: u32, daemon: u32 },

    #[error("manifest locked — another process is writing")]
    ManifestLocked,

    #[error("spawn failed: {0}")]
    SpawnFailed(String),

    #[error("kill target required — use --agent, --worktree, or --all")]
    KillTargetRequired,

    #[error("invalid argument: {0}")]
    InvalidArgument(String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("yaml error: {0}")]
    Yaml(#[from] serde_yml::Error),
}

impl PuError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::NotInitialized => "NOT_INITIALIZED",
            Self::AlreadyInitialized(_) => "ALREADY_INITIALIZED",
            Self::AgentNotFound(_) => "AGENT_NOT_FOUND",
            Self::WorktreeNotFound(_) => "WORKTREE_NOT_FOUND",
            Self::DaemonNotRunning => "DAEMON_NOT_RUNNING",
            Self::DaemonConnectionFailed(_) => "DAEMON_CONNECTION_FAILED",
            Self::ProtocolMismatch { .. } => "PROTOCOL_MISMATCH",
            Self::ManifestLocked => "MANIFEST_LOCKED",
            Self::SpawnFailed(_) => "SPAWN_FAILED",
            Self::KillTargetRequired => "KILL_TARGET_REQUIRED",
            Self::InvalidArgument(_) => "INVALID_ARGUMENT",
            Self::Io(_) => "IO_ERROR",
            Self::Json(_) => "JSON_ERROR",
            Self::Yaml(_) => "YAML_ERROR",
        }
    }

    pub fn exit_code(&self) -> i32 {
        match self {
            Self::NotInitialized => 2,
            Self::AgentNotFound(_) | Self::WorktreeNotFound(_) => 3,
            Self::DaemonNotRunning | Self::DaemonConnectionFailed(_) => 4,
            Self::ProtocolMismatch { .. } => 5,
            _ => 1,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_not_initialized_should_have_code_and_exit() {
        let err = PuError::NotInitialized;
        assert_eq!(err.code(), "NOT_INITIALIZED");
        assert_eq!(err.exit_code(), 2);
    }

    #[test]
    fn given_not_initialized_should_display_helpful_message() {
        let err = PuError::NotInitialized;
        let msg = format!("{err}");
        assert!(msg.contains("pu init"), "expected init hint, got: {msg}");
    }

    #[test]
    fn given_agent_not_found_should_include_id_in_message() {
        let err = PuError::AgentNotFound("ag-abc".into());
        let msg = format!("{err}");
        assert!(msg.contains("ag-abc"));
        assert_eq!(err.code(), "AGENT_NOT_FOUND");
        assert_eq!(err.exit_code(), 3);
    }

    #[test]
    fn given_worktree_not_found_should_include_id_in_message() {
        let err = PuError::WorktreeNotFound("wt-xyz".into());
        assert_eq!(err.code(), "WORKTREE_NOT_FOUND");
        assert_eq!(err.exit_code(), 3);
    }

    #[test]
    fn given_daemon_not_running_should_have_exit_code_4() {
        let err = PuError::DaemonNotRunning;
        assert_eq!(err.code(), "DAEMON_NOT_RUNNING");
        assert_eq!(err.exit_code(), 4);
    }

    #[test]
    fn given_protocol_mismatch_should_include_versions() {
        let err = PuError::ProtocolMismatch { client: 1, daemon: 2 };
        let msg = format!("{err}");
        assert!(msg.contains("1") && msg.contains("2"));
        assert_eq!(err.code(), "PROTOCOL_MISMATCH");
        assert_eq!(err.exit_code(), 5);
    }

    #[test]
    fn given_io_error_should_convert_from_std() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "gone");
        let err: PuError = io_err.into();
        assert_eq!(err.code(), "IO_ERROR");
        assert_eq!(err.exit_code(), 1);
    }

    #[test]
    fn given_kill_target_required_should_hint_flags() {
        let err = PuError::KillTargetRequired;
        let msg = format!("{err}");
        assert!(msg.contains("--agent") || msg.contains("--all"));
    }

    #[test]
    fn given_spawn_failed_should_include_reason() {
        let err = PuError::SpawnFailed("PTY allocation failed".into());
        let msg = format!("{err}");
        assert!(msg.contains("PTY allocation failed"));
        assert_eq!(err.code(), "SPAWN_FAILED");
    }
}
