use std::path::Path;
use pu_core::protocol::Request;
use crate::client;
use crate::daemon_ctrl;
use crate::error::CliError;
use crate::output;

pub async fn run(socket: &Path, json: bool) -> Result<(), CliError> {
    daemon_ctrl::ensure_daemon(socket).await?;

    let resp = client::send_request(socket, &Request::Health).await?;
    let resp = output::check_response(resp, json)?;
    output::print_response(&resp, json);
    Ok(())
}
