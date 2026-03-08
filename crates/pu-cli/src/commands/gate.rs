use std::path::Path;

use crate::client;
use crate::commands;
use crate::daemon_ctrl;
use crate::error::CliError;
use pu_core::protocol::{Request, Response};

pub async fn run(socket: &Path, event: &str, project_root: Option<String>) -> Result<(), CliError> {
    daemon_ctrl::ensure_daemon(socket).await?;
    let project_root = match project_root {
        Some(pr) => pr,
        None => commands::cwd_string()?,
    };

    // For git hooks, worktree_path is the same as project_root (the hook runs inside the worktree)
    let worktree_path = project_root.clone();

    let resp = client::send_request(
        socket,
        &Request::EvaluateGate {
            event: event.to_string(),
            project_root,
            worktree_path,
        },
    )
    .await?;

    match resp {
        Response::GateResult { passed, output } => {
            if !output.is_empty() {
                eprint!("{output}");
            }
            if passed {
                Ok(())
            } else {
                Err(CliError::Other("gate check failed".into()))
            }
        }
        Response::Error { code, message } => Err(CliError::DaemonError { code, message }),
        _ => Err(CliError::Other("unexpected response".into())),
    }
}
