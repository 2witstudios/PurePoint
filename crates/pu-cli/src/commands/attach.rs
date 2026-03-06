use crate::client;
use crate::daemon_ctrl;
use crate::error::CliError;
use crate::output;
use pu_core::protocol::Request;
use std::path::Path;

pub async fn run(socket: &Path, agent_id: &str) -> Result<(), CliError> {
    daemon_ctrl::ensure_daemon(socket).await?;

    // TODO: Implement raw PTY relay mode
    // For now, send attach request and print buffered output
    let resp = client::send_request(
        socket,
        &Request::Attach {
            agent_id: agent_id.to_string(),
        },
    )
    .await?;
    let resp = output::check_response(resp, false)?;
    output::print_response(&resp, false);
    Ok(())
}
