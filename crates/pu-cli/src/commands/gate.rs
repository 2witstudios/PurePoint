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
        None => commands::project_root_string()?,
    };

    // Git hooks run inside the worktree directory (cwd), while project_root points to
    // where .pu/triggers/ lives. Gate commands should execute in the worktree.
    let worktree_path = commands::cwd_string()?;

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
