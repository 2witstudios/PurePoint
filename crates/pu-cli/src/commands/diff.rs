use crate::client;
use crate::daemon_ctrl;
use crate::error::CliError;
use crate::output;
use pu_core::protocol::Request;
use std::path::Path;

pub async fn run(
    socket: &Path,
    worktree: Option<String>,
    stat: bool,
    json: bool,
) -> Result<(), CliError> {
    daemon_ctrl::ensure_daemon(socket).await?;

    let project_root = crate::commands::cwd_string()?;
    let resp = client::send_request(
        socket,
        &Request::Diff {
            project_root,
            worktree_id: worktree,
            stat,
        },
    )
    .await?;
    let resp = output::check_response(resp, json)?;
    output::print_response(&resp, json);
    Ok(())
}
