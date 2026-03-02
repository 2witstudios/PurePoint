use std::path::Path;
use pu_core::protocol::{Request, Response};
use crate::client;
use crate::daemon_ctrl;
use crate::error::CliError;
use crate::output;

pub async fn run(socket: &Path) -> Result<(), CliError> {
    daemon_ctrl::ensure_daemon(socket).await?;

    let resp = client::send_request(socket, &Request::Health).await?;
    if let Response::Error { code, message } = resp {
        return Err(CliError::DaemonError { code, message });
    }
    output::print_response(&resp, false);
    Ok(())
}
