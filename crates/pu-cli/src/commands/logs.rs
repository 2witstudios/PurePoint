use std::path::Path;
use pu_core::protocol::Request;
use crate::client;
use crate::daemon_ctrl;
use crate::error::CliError;
use crate::output;

pub async fn run(
    socket: &Path,
    agent_id: &str,
    tail: usize,
    json: bool,
) -> Result<(), CliError> {
    daemon_ctrl::ensure_daemon(socket).await?;

    let resp = client::send_request(
        socket,
        &Request::Logs {
            agent_id: agent_id.to_string(),
            tail,
        },
    )
    .await?;
    let resp = output::check_response(resp, json)?;
    output::print_response(&resp, json);
    Ok(())
}
