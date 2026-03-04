use crate::client;
use crate::daemon_ctrl;
use crate::error::CliError;
use crate::output;
use pu_core::protocol::{Request, Response};
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
    if let Response::Error { code, message } = &resp {
        return Err(CliError::DaemonError {
            code: code.clone(),
            message: message.clone(),
        });
    }
    output::print_response(&resp, false);
    Ok(())
}
