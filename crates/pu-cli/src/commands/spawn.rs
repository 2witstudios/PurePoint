use std::path::Path;
use pu_core::protocol::{Request, Response};
use crate::client;
use crate::daemon_ctrl;
use crate::error::CliError;
use crate::output;

pub async fn run(
    socket: &Path,
    prompt: String,
    agent: Option<String>,
    name: Option<String>,
    base: Option<String>,
    root: bool,
    worktree: Option<String>,
) -> Result<(), CliError> {
    daemon_ctrl::ensure_daemon(socket).await?;

    let cwd = std::env::current_dir()?;
    let project_root = cwd.to_string_lossy().to_string();
    let resp = client::send_request(
        socket,
        &Request::Spawn {
            project_root,
            prompt,
            agent: agent.unwrap_or_else(|| "claude".into()),
            name,
            base,
            root,
            worktree,
        },
    )
    .await?;
    if let Response::Error { code, message } = resp {
        return Err(CliError::DaemonError { code, message });
    }
    output::print_response(&resp, false);
    Ok(())
}
