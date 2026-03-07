use crate::client;
use crate::daemon_ctrl;
use crate::error::CliError;
use crate::output;
use pu_core::protocol::{KillTarget, Request};
use std::path::Path;

pub async fn run(
    socket: &Path,
    agent: Option<String>,
    worktree: Option<String>,
    all: bool,
    include_root: bool,
    json: bool,
) -> Result<(), CliError> {
    let self_agent_id = std::env::var("PU_AGENT_ID").ok();

    let target = if all {
        if include_root {
            KillTarget::All
        } else {
            KillTarget::AllWorktrees
        }
    } else if let Some(a) = agent {
        // Self-protection: refuse to kill own agent
        if let Some(ref self_id) = self_agent_id {
            if &a == self_id {
                return Err(CliError::Other("cannot kill self".into()));
            }
        }
        KillTarget::Agent(a)
    } else if let Some(wt) = worktree {
        KillTarget::Worktree(wt)
    } else {
        return Err(CliError::Other(
            "kill target required — use --agent, --worktree, or --all".into(),
        ));
    };

    // Auto-exclude self from bulk kills
    let exclude = match self_agent_id {
        Some(id) if !matches!(target, KillTarget::Agent(_)) => vec![id],
        _ => vec![],
    };

    daemon_ctrl::ensure_daemon(socket).await?;

    let project_root = crate::commands::cwd_string()?;
    let resp = client::send_request(
        socket,
        &Request::Kill {
            project_root,
            target,
            exclude,
        },
    )
    .await?;
    let resp = output::check_response(resp, json)?;
    output::print_response(&resp, json);
    Ok(())
}
