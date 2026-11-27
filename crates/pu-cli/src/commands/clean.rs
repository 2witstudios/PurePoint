use crate::client;
use crate::daemon_ctrl;
use crate::error::CliError;
use crate::output;
use pu_core::protocol::{Request, Response};
use std::path::Path;

pub async fn run(
    socket: &Path,
    worktree: Option<String>,
    all: bool,
    json: bool,
) -> Result<(), CliError> {
    if !all && worktree.is_none() {
        return Err(CliError::Other(
            "clean target required — use --worktree <id> or --all".into(),
        ));
    }

    daemon_ctrl::ensure_daemon(socket).await?;
    let project_root = crate::commands::project_root_string()?;

    if all {
        // Get status to find all worktree IDs
        let status_resp = client::send_request(
            socket,
            &Request::Status {
                project_root: project_root.clone(),
                agent_id: None,
            },
        )
        .await?;
        let status_resp = output::check_response(status_resp, json)?;

        let wt_ids: Vec<String> = match &status_resp {
            Response::StatusReport { worktrees, .. } => {
                worktrees.iter().map(|wt| wt.id.clone()).collect()
            }
            _ => vec![],
        };

        if wt_ids.is_empty() {
            if json {
                println!("[]");
            } else {
                println!("No worktrees to clean");
            }
            return Ok(());
        }

        // `--json` prints one array (as for no worktrees), not one document
        // per worktree, so the output parses as JSON.
        let mut results = Vec::new();
        for wt_id in &wt_ids {
            let resp = client::send_request(
                socket,
                &Request::DeleteWorktree {
                    project_root: project_root.clone(),
                    worktree_id: wt_id.clone(),
                },
            )
            .await?;
            let resp = output::check_response(resp, json)?;
            if json {
                results.push(resp);
            } else {
                output::print_response(&resp, false)?;
            }
        }
        if json {
            println!("{}", json_array(&results)?);
        }
    } else if let Some(wt_id) = worktree {
        let resp = client::send_request(
            socket,
            &Request::DeleteWorktree {
                project_root,
                worktree_id: wt_id,
            },
        )
        .await?;
        let resp = output::check_response(resp, json)?;
        output::print_response(&resp, json)?;
    }

    Ok(())
}

fn json_array(results: &[Response]) -> Result<String, CliError> {
    Ok(serde_json::to_string_pretty(results)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_several_cleaned_worktrees_json_should_be_one_array() {
        // given
        let result = |id: &str| Response::DeleteWorktreeResult {
            worktree_id: id.into(),
            killed_agents: vec![],
            branch_deleted: true,
            remote_deleted: false,
            directory_removed: true,
            error: None,
        };

        // when
        let out = json_array(&[result("wt-1"), result("wt-2")]).expect("serialize");

        // then
        let parsed: serde_json::Value = serde_json::from_str(&out).expect("valid JSON");
        let items = parsed.as_array().expect("array");
        assert_eq!(items.len(), 2);
        assert_eq!(items[0]["type"], "delete_worktree_result");
        assert_eq!(items[1]["worktree_id"], "wt-2");
    }
}
