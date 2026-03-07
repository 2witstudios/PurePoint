use std::path::Path;

use crate::client;
use crate::commands;
use crate::daemon_ctrl;
use crate::error::CliError;
use crate::output;
use pu_core::protocol::Request;
use pu_core::template;

/// List templates via daemon. Falls back to local-only read if daemon is unavailable.
pub async fn run_list(socket: &Path, json: bool) -> Result<(), CliError> {
    // Try daemon first
    if daemon_ctrl::check_daemon_health(socket).await {
        let project_root = commands::cwd_string()?;
        let resp = client::send_request(socket, &Request::ListTemplates { project_root }).await?;
        let resp = output::check_response(resp, json)?;
        output::print_response(&resp, json);
        return Ok(());
    }

    // Fallback: read templates locally
    run_list_local(json)
}

/// Local-only template listing (no daemon required).
fn run_list_local(json: bool) -> Result<(), CliError> {
    let cwd = std::env::current_dir()?;
    let templates = template::list_templates(&cwd);

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&templates).expect("templates JSON serialization failed")
        );
        return Ok(());
    }

    if templates.is_empty() {
        println!("No templates found");
        println!("  Local:  .pu/templates/*.md");
        println!("  Global: ~/.pu/templates/*.md");
        return Ok(());
    }

    for tpl in &templates {
        let desc = if tpl.description.is_empty() {
            String::new()
        } else {
            format!(" — {}", tpl.description)
        };
        let vars = template::extract_variables(&tpl.body);
        let var_str = if vars.is_empty() {
            String::new()
        } else {
            format!(" [{}]", vars.join(", "))
        };
        println!("  {} ({}){}{}", tpl.name, tpl.source, desc, var_str);
    }

    Ok(())
}

pub async fn run_show(socket: &Path, name: &str, json: bool) -> Result<(), CliError> {
    daemon_ctrl::ensure_daemon(socket).await?;
    let project_root = commands::cwd_string()?;
    let resp = client::send_request(
        socket,
        &Request::GetTemplate {
            project_root,
            name: name.to_string(),
        },
    )
    .await?;
    let resp = output::check_response(resp, json)?;
    output::print_response(&resp, json);
    Ok(())
}

pub async fn run_create(
    socket: &Path,
    name: &str,
    body: &str,
    description: &str,
    agent: &str,
    scope: &str,
    json: bool,
) -> Result<(), CliError> {
    daemon_ctrl::ensure_daemon(socket).await?;
    let project_root = commands::cwd_string()?;
    let resp = client::send_request(
        socket,
        &Request::SaveTemplate {
            project_root,
            name: name.to_string(),
            description: description.to_string(),
            agent: agent.to_string(),
            body: body.to_string(),
            scope: scope.to_string(),
            command: None,
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
        &Request::DeleteTemplate {
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
