use crate::client;
use crate::commands::cwd_string;
use crate::daemon_ctrl;
use crate::error::CliError;
use crate::output;
use pu_core::protocol::Request;
use std::path::Path;

pub async fn run(socket: &Path, agent: Option<String>, json: bool) -> Result<(), CliError> {
    daemon_ctrl::ensure_daemon(socket).await?;

    let project_root = cwd_string()?;
    let resp = client::send_request(
        socket,
        &Request::Status {
            project_root,
            agent_id: agent,
        },
    )
    .await?;
    let resp = output::check_response(resp, json)?;
    output::print_response(&resp, json);
    Ok(())
}
