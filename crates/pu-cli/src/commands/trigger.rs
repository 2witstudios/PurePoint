use std::path::Path;

use crate::client;
use crate::commands;
use crate::daemon_ctrl;
use crate::error::CliError;
use crate::output;
use pu_core::protocol::{GatePayload, Request, TriggerActionPayload};

pub async fn run_list(socket: &Path, json: bool) -> Result<(), CliError> {
    daemon_ctrl::ensure_daemon(socket).await?;
    let project_root = commands::cwd_string()?;
    let resp = client::send_request(socket, &Request::ListTriggers { project_root }).await?;
    let resp = output::check_response(resp, json)?;
    output::print_response(&resp, json)?;
    Ok(())
}

pub async fn run_show(socket: &Path, name: &str, json: bool) -> Result<(), CliError> {
    daemon_ctrl::ensure_daemon(socket).await?;
    let project_root = commands::cwd_string()?;
    let resp = client::send_request(
        socket,
        &Request::GetTrigger {
            project_root,
            name: name.to_string(),
        },
    )
    .await?;
    let resp = output::check_response(resp, json)?;
    output::print_response(&resp, json)?;
    Ok(())
}

pub struct CreateTriggerParams {
    pub name: String,
    pub event: String,
    pub description: Option<String>,
    pub injects: Vec<String>,
    pub gates: Vec<String>,
    pub scope: String,
    pub json: bool,
}

pub async fn run_create(socket: &Path, params: CreateTriggerParams) -> Result<(), CliError> {
    daemon_ctrl::ensure_daemon(socket).await?;
    let project_root = commands::cwd_string()?;

    // Build sequence from --inject and --gate flags
    let mut sequence: Vec<TriggerActionPayload> = Vec::new();
    for text in &params.injects {
        sequence.push(TriggerActionPayload {
            inject: Some(text.clone()),
            gate: None,
            max_retries: None,
        });
    }
    for cmd in &params.gates {
        sequence.push(TriggerActionPayload {
            inject: None,
            gate: Some(GatePayload {
                run: cmd.clone(),
                expect_exit: None,
            }),
            max_retries: None,
        });
    }

    if sequence.is_empty() {
        return Err(CliError::Other(
            "at least one --inject or --gate is required".into(),
        ));
    }

    let resp = client::send_request(
        socket,
        &Request::SaveTrigger {
            project_root,
            name: params.name,
            description: params.description,
            on: params.event,
            sequence,
            variables: std::collections::HashMap::new(),
            scope: params.scope,
        },
    )
    .await?;
    let resp = output::check_response(resp, params.json)?;
    output::print_response(&resp, params.json)?;
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
        &Request::DeleteTrigger {
            project_root,
            name: name.to_string(),
            scope: scope.to_string(),
        },
    )
    .await?;
    let resp = output::check_response(resp, json)?;
    output::print_response(&resp, json)?;
    Ok(())
}
