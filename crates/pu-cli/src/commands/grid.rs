use std::path::Path;

use pu_core::protocol::{GridCommand, Request, Response};

use crate::GridAction;
use crate::client;
use crate::daemon_ctrl;
use crate::error::CliError;
use crate::output;

pub async fn run(socket: &Path, action: GridAction) -> Result<(), CliError> {
    daemon_ctrl::ensure_daemon(socket).await?;

    let project_root = crate::commands::cwd_string()?;

    match action {
        GridAction::Show { json } => {
            let resp = client::send_request(
                socket,
                &Request::GridCommand {
                    project_root,
                    command: GridCommand::GetLayout,
                },
            )
            .await?;
            let resp = output::check_response(resp, json)?;

            match resp {
                Response::GridLayout { layout } => {
                    if json {
                        println!(
                            "{}",
                            serde_json::to_string_pretty(&layout)
                                .expect("layout JSON serialization failed")
                        );
                    } else {
                        print_ascii_grid(&layout);
                    }
                }
                _ => {
                    println!("No grid layout");
                }
            }
        }

        GridAction::Split { axis, leaf } => {
            let resp = client::send_request(
                socket,
                &Request::GridCommand {
                    project_root,
                    command: GridCommand::Split {
                        leaf_id: leaf,
                        axis,
                    },
                },
            )
            .await?;
            output::check_response(resp, false)?;
            println!("Split pane");
        }

        GridAction::Close { leaf } => {
            let resp = client::send_request(
                socket,
                &Request::GridCommand {
                    project_root,
                    command: GridCommand::Close { leaf_id: leaf },
                },
            )
            .await?;
            output::check_response(resp, false)?;
            println!("Closed pane");
        }

        GridAction::Focus { direction, leaf } => {
            let resp = client::send_request(
                socket,
                &Request::GridCommand {
                    project_root,
                    command: GridCommand::Focus {
                        leaf_id: leaf,
                        direction,
                    },
                },
            )
            .await?;
            output::check_response(resp, false)?;
            println!("Focus moved");
        }

        GridAction::Assign { agent_id, leaf } => {
            let leaf_id = leaf.unwrap_or(0);
            let resp = client::send_request(
                socket,
                &Request::GridCommand {
                    project_root,
                    command: GridCommand::SetAgent { leaf_id, agent_id },
                },
            )
            .await?;
            output::check_response(resp, false)?;
            println!("Agent assigned");
        }
    }

    Ok(())
}

/// Render the grid layout as an ASCII table.
fn print_ascii_grid(layout: &serde_json::Value) {
    if layout.is_null() {
        println!("No grid layout");
        return;
    }

    // Collect all leaves from the layout JSON
    let mut leaves = Vec::new();
    collect_leaves(layout, &mut leaves);

    if leaves.is_empty() {
        println!("Empty grid");
        return;
    }

    // Simple rendering: show leaves in a box
    let max_width = 28;
    let border_h = "─".repeat(max_width);
    println!("┌{border_h}┐");
    for leaf in &leaves {
        let agent = leaf.as_deref().unwrap_or("(empty)");
        let padded = format!("{agent:^max_width$}");
        println!("│{padded}│");
    }
    println!("└{border_h}┘");
}

fn collect_leaves(node: &serde_json::Value, out: &mut Vec<Option<String>>) {
    match node.get("type").and_then(|t| t.as_str()) {
        Some("leaf") => {
            let agent_id = node
                .get("agentId")
                .and_then(|a| a.as_str())
                .map(String::from);
            out.push(agent_id);
        }
        Some("split") => {
            if let Some(first) = node.get("first") {
                collect_leaves(first, out);
            }
            if let Some(second) = node.get("second") {
                collect_leaves(second, out);
            }
        }
        _ => {}
    }
}
