use std::path::Path;
use pu_core::protocol::{KillTarget, Request, Response};
use crate::client;
use crate::daemon_ctrl;
use crate::error::CliError;
use crate::output;

pub async fn run(
    socket: &Path,
    agent: Option<String>,
    worktree: Option<String>,
    all: bool,
) -> Result<(), CliError> {
    let target = if all {
        KillTarget::All
    } else if let Some(a) = agent {
        KillTarget::Agent(a)
    } else if let Some(wt) = worktree {
        KillTarget::Worktree(wt)
    } else {
        return Err(CliError::Other(
            "kill target required — use --agent, --worktree, or --all".into(),
        ));
    };

    daemon_ctrl::ensure_daemon(socket).await?;

    let cwd = std::env::current_dir()?;
    let project_root = cwd.to_string_lossy().to_string();
    let resp = client::send_request(socket, &Request::Kill { project_root, target }).await?;
    if let Response::Error { code, message } = resp {
        return Err(CliError::DaemonError { code, message });
    }
    output::print_response(&resp, false);
    Ok(())
}
