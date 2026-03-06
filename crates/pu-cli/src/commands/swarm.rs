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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_roster_valid_three_parts() {
        let input = vec!["reviewer:review:2".to_string()];
        let result = parse_roster(&input).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].agent_def, "reviewer");
        assert_eq!(result[0].role, "review");
        assert_eq!(result[0].quantity, 2);
    }

    #[test]
    fn parse_roster_valid_two_parts_default_qty() {
        let input = vec!["builder:build".to_string()];
        let result = parse_roster(&input).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].agent_def, "builder");
        assert_eq!(result[0].role, "build");
        assert_eq!(result[0].quantity, 1);
    }

    #[test]
    fn parse_roster_missing_fields() {
        let input = vec!["justname".to_string()];
        let result = parse_roster(&input);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("justname"),
            "error should mention the bad input"
        );
    }

    #[test]
    fn parse_roster_bad_quantity() {
        let input = vec!["agent:role:abc".to_string()];
        let result = parse_roster(&input);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("invalid quantity"),
            "error should mention invalid quantity"
        );
    }

    #[test]
    fn parse_roster_zero_quantity() {
        let input = vec!["agent:role:0".to_string()];
        let result = parse_roster(&input).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].quantity, 0);
    }

    #[test]
    fn parse_roster_empty_input() {
        let input: Vec<String> = vec![];
        let result = parse_roster(&input).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn parse_roster_multiple_entries() {
        let input = vec!["a:b:1".to_string(), "c:d:3".to_string()];
        let result = parse_roster(&input).unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].agent_def, "a");
        assert_eq!(result[0].role, "b");
        assert_eq!(result[0].quantity, 1);
        assert_eq!(result[1].agent_def, "c");
        assert_eq!(result[1].role, "d");
        assert_eq!(result[1].quantity, 3);
    }
}
