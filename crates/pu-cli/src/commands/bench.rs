use crate::client;
use crate::daemon_ctrl;
use crate::error::CliError;
use crate::output;
use pu_core::protocol::{Request, SuspendTarget};
use std::path::Path;

pub async fn run_bench(
    socket: &Path,
    agent: Option<String>,
    all: bool,
    json: bool,
) -> Result<(), CliError> {
    let target = if all {
        SuspendTarget::All
    } else if let Some(id) = agent {
        SuspendTarget::Agent(id)
    } else {
        return Err(CliError::Other(
            "bench target required — use <AGENT_ID> or --all".into(),
        ));
    };

    daemon_ctrl::ensure_daemon(socket).await?;

    let project_root = crate::commands::cwd_string()?;
    let resp = client::send_request(
        socket,
        &Request::Suspend {
            project_root,
            target,
        },
    )
    .await?;
    let resp = output::check_response(resp, json)?;
    output::print_response(&resp, json);
    Ok(())
}

pub async fn run_play(socket: &Path, agent_id: &str, json: bool) -> Result<(), CliError> {
    daemon_ctrl::ensure_daemon(socket).await?;

    let project_root = crate::commands::cwd_string()?;
    let resp = client::send_request(
        socket,
        &Request::Resume {
            project_root,
            agent_id: agent_id.to_string(),
        },
    )
    .await?;
    let resp = output::check_response(resp, json)?;
    output::print_response(&resp, json);
    Ok(())
}
