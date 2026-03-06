use std::collections::HashMap;
use std::path::Path;

use crate::client;
use crate::commands;
use crate::daemon_ctrl;
use crate::error::CliError;
use crate::output;
use pu_core::protocol::Request;

/// Parse --var KEY=VALUE pairs into a HashMap (reuses spawn.rs logic).
fn parse_vars(vars: &[String]) -> Result<HashMap<String, String>, CliError> {
    let mut map = HashMap::new();
    for v in vars {
        let (key, value) = v.split_once('=').ok_or_else(|| {
            CliError::Other(format!("invalid --var format: {v} (expected KEY=VALUE)"))
        })?;
        map.insert(key.to_string(), value.to_string());
    }
    Ok(map)
}

pub async fn run_list(socket: &Path, json: bool) -> Result<(), CliError> {
    daemon_ctrl::ensure_daemon(socket).await?;
    let project_root = commands::cwd_string()?;
    let resp = client::send_request(
        socket,
        &Request::ListSwarmDefs { project_root },
    )
    .await?;
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
    include_terminal: bool,
    scope: &str,
    json: bool,
) -> Result<(), CliError> {
    daemon_ctrl::ensure_daemon(socket).await?;
    let project_root = commands::cwd_string()?;
    let resp = client::send_request(
        socket,
        &Request::SaveSwarmDef {
            project_root,
            name: name.to_string(),
            worktree_count: worktrees,
            worktree_template: worktree_template.to_string(),
            roster: Vec::new(),
            include_terminal,
            scope: scope.to_string(),
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
    let var_map = parse_vars(&vars)?;
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

    // --- parse_vars (same logic as spawn, but local to swarm) ---

    #[test]
    fn given_valid_key_value_pairs_should_parse() {
        let vars = vec!["FOO=bar".to_string(), "BAZ=qux".to_string()];
        let map = parse_vars(&vars).unwrap();
        assert_eq!(map.len(), 2);
        assert_eq!(map["FOO"], "bar");
        assert_eq!(map["BAZ"], "qux");
    }

    #[test]
    fn given_empty_vars_should_return_empty_map() {
        let vars: Vec<String> = vec![];
        let map = parse_vars(&vars).unwrap();
        assert!(map.is_empty());
    }

    #[test]
    fn given_value_with_equals_should_preserve_remainder() {
        let vars = vec!["KEY=val=ue=extra".to_string()];
        let map = parse_vars(&vars).unwrap();
        assert_eq!(map["KEY"], "val=ue=extra");
    }

    #[test]
    fn given_missing_equals_should_return_error() {
        let vars = vec!["NOEQUALS".to_string()];
        let result = parse_vars(&vars);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("invalid --var format"));
    }
}
