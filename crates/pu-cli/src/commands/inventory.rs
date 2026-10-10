use crate::{InventoryAction, client, daemon_ctrl, error::CliError, output};
use pu_core::protocol::{AgentQueryState, InventoryKind, Request};
use std::path::Path;

pub async fn run_list(
    socket: &Path,
    kind: InventoryKind,
    action: InventoryAction,
) -> Result<(), CliError> {
    let InventoryAction::List { state, json, .. } = action;
    if state.is_some() && kind != InventoryKind::Agents {
        return Err(CliError::Other(
            "--state is only supported by agents list".into(),
        ));
    }
    let state = state.map(|s| match s.as_str() {
        "running" => AgentQueryState::Running,
        "suspended" => AgentQueryState::Suspended,
        "broken" => AgentQueryState::Broken,
        _ => AgentQueryState::Unknown,
    });
    run(socket, kind, state, json).await
}

pub async fn run(
    socket: &Path,
    kind: InventoryKind,
    state: Option<AgentQueryState>,
    json: bool,
) -> Result<(), CliError> {
    daemon_ctrl::ensure_daemon(socket).await?;
    let response = client::send_request(
        socket,
        &Request::Inventory {
            // Inventory is global by default, even inside a builder's worktree.
            project_root: crate::commands::explicit_project(),
            kind,
            state,
        },
    )
    .await?;
    let response = output::check_response(response, json)?;
    output::print_response(&response, json)
}
