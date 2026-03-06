use std::path::Path;

use crate::client;
use crate::commands;
use crate::daemon_ctrl;
use crate::error::CliError;
use crate::output;
use pu_core::protocol::{Request, SwarmRosterEntryPayload};

pub async fn run_list(socket: &Path, json: bool) -> Result<(), CliError> {
    daemon_ctrl::ensure_daemon(socket).await?;
    let project_root = commands::cwd_string()?;
    let resp = client::send_request(socket, &Request::ListSwarmDefs { project_root }).await?;
    let resp = output::check_response(resp, json)?;
    output::print_response(&resp, json);
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub async fn run_create(
    socket: &Path,
    name: &str,
    worktrees: u32,
    worktree_template: &str,
    roster_args: Vec<String>,
    include_terminal: bool,
    scope: &str,
    json: bool,
) -> Result<(), CliError> {
    daemon_ctrl::ensure_daemon(socket).await?;
    let project_root = commands::cwd_string()?;
    let roster = parse_roster(&roster_args)?;
    let resp = client::send_request(
        socket,
        &Request::SaveSwarmDef {
            project_root,
            name: name.to_string(),
            worktree_count: worktrees,
            worktree_template: worktree_template.to_string(),
            roster,
            include_terminal,
            scope: scope.to_string(),
        },
    )
    .await?;
    let resp = output::check_response(resp, json)?;
    output::print_response(&resp, json);
    Ok(())
}

/// Parse --roster "agent_def:role:quantity" entries.
fn parse_roster(entries: &[String]) -> Result<Vec<SwarmRosterEntryPayload>, CliError> {
    entries
        .iter()
        .map(|e| {
            let parts: Vec<&str> = e.splitn(3, ':').collect();
            if parts.len() < 2 {
                return Err(CliError::Other(format!(
                    "invalid --roster format: {e} (expected AGENT_DEF:ROLE or AGENT_DEF:ROLE:QTY)"
                )));
            }
            let quantity = if parts.len() == 3 {
                parts[2]
                    .parse::<u32>()
                    .map_err(|_| CliError::Other(format!("invalid quantity in --roster: {e}")))?
            } else {
                1
            };
            Ok(SwarmRosterEntryPayload {
                agent_def: parts[0].to_string(),
                role: parts[1].to_string(),
                quantity,
            })
        })
        .collect()
}

pub async fn run_show(socket: &Path, name: &str, json: bool) -> Result<(), CliError> {
    daemon_ctrl::ensure_daemon(socket).await?;
    let project_root = commands::cwd_string()?;
    let resp = client::send_request(
        socket,
        &Request::GetSwarmDef {
            project_root,
            name: name.to_string(),
        },
    )
    .await?;
    let resp = output::check_response(resp, json)?;
    output::print_response(&resp, json);
    Ok(())
}

pub async fn run_delete(
    socket: &Path,
    name: &str,
    scope: &str,
    json: bool,
) -> Result<(), CliError> {
    daemon_ctrl::ensure_daemon(socket).await?;
    let project_root = commands::cwd_string()?;
    let resp = client::send_request(
        socket,
        &Request::DeleteSwarmDef {
            project_root,
            name: name.to_string(),
            scope: scope.to_string(),
        },
    )
    .await?;
    let resp = output::check_response(resp, json)?;
    output::print_response(&resp, json);
    Ok(())
}

pub async fn run_run(
    socket: &Path,
    name: &str,
    vars: Vec<String>,
    json: bool,
) -> Result<(), CliError> {
    daemon_ctrl::ensure_daemon(socket).await?;
    let project_root = commands::cwd_string()?;
    let var_map = commands::parse_vars(&vars)?;
    let resp = client::send_request(
        socket,
        &Request::RunSwarm {
            project_root,
            swarm_name: name.to_string(),
            vars: var_map,
        },
    )
    .await?;
    let resp = output::check_response(resp, json)?;
    output::print_response(&resp, json);
    Ok(())
}
