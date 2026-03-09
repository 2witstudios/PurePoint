use crate::client;
use crate::daemon_ctrl;
use crate::error::CliError;
use crate::output;
use pu_core::protocol::{Request, SuspendTarget};
use std::path::Path;

/// Suspend one or all agents ("bench" them).
///
/// When `--all` is used, the invoking agent may be included in the suspend
/// results. A warning is emitted if this happens; use `pu play <id>` to resume.
pub async fn run_bench(
    socket: &Path,
    agent: Option<String>,
    all: bool,
    json: bool,
) -> Result<(), CliError> {
    let self_agent_id = std::env::var("PU_AGENT_ID").ok();

    let target = if all {
        SuspendTarget::All
    } else if let Some(id) = agent {
        // Self-protection: refuse to bench own agent
        if let Some(ref self_id) = self_agent_id {
            if &id == self_id {
                return Err(CliError::Other("cannot bench self".into()));
            }
        }
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

    // Warn if self was included in bulk bench results
    if let Some(ref self_id) = self_agent_id {
        if let pu_core::protocol::Response::SuspendResult { ref suspended } = resp {
            if suspended.contains(self_id) {
                eprintln!(
                    "warning: agent {self_id} benched itself — use `pu play {self_id}` to resume",
                );
            }
        }
    }

    output::print_response(&resp, json)?;
    Ok(())
}

/// Resume a previously benched agent, putting it back in play.
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
    output::print_response(&resp, json)?;
    Ok(())
}
