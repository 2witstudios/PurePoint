use std::path::Path;
use pu_core::protocol::{Request, Response};
use crate::client;
use crate::daemon_ctrl;
use crate::error::CliError;
use crate::output;

pub async fn run(
    socket: &Path,
    agent: Option<String>,
    json: bool,
) -> Result<(), CliError> {
    daemon_ctrl::ensure_daemon(socket).await?;

    let cwd = std::env::current_dir()?;
    let project_root = cwd.to_string_lossy().to_string();
    let resp = client::send_request(
        socket,
        &Request::Status {
            project_root,
            agent_id: agent,
        },
    )
    .await?;
    if let Response::Error { code, message } = resp {
        if json {
            // Machine consumers need the error as JSON on stdout
            output::print_response(&Response::Error { code: code.clone(), message: message.clone() }, true);
        }
        return Err(CliError::DaemonError { code, message });
    }
    output::print_response(&resp, json);
    Ok(())
}
