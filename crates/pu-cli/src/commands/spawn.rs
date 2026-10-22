use crate::client;
use crate::commands;
use crate::daemon_ctrl;
use crate::error::CliError;
use crate::output;
use pu_core::protocol::{Request, Response};
use pu_core::template;
use std::collections::HashMap;
use std::path::Path;

#[allow(clippy::too_many_arguments)]
pub async fn run(
    socket: &Path,
    prompt: Option<String>,
    agent: Option<String>,
    name: Option<String>,
    base: Option<String>,
    root: bool,
    worktree: Option<String>,
    template_name: Option<String>,
    file: Option<String>,
    command: Option<String>,
    vars: Vec<String>,
    no_auto: bool,
    agent_args: Option<String>,
    plan_mode: bool,
    no_trigger: bool,
    trigger: Option<String>,
    json: bool,
) -> Result<(), CliError> {
    daemon_ctrl::ensure_daemon(socket).await?;

    let project_root = commands::project_root_string()?;
    let project_root_path = std::path::PathBuf::from(&project_root);

    // Parse --var KEY=VALUE pairs
    let var_map = commands::parse_vars(&vars)?;

    // A machine's templates live on that machine, so ask its daemon.
    let remote_template = match &template_name {
        Some(name) if crate::machine_target::is_remote() => {
            Some(fetch_template(socket, &project_root, name).await?)
        }
        _ => None,
    };
    let find_template = |name: &str| match remote_template {
        Some(template) => Some(template),
        None => template::find_template(&project_root_path, name),
    };

    // Resolve prompt + agent override + command override from template/file/inline
    let (resolved_prompt, agent_override, command_override) =
        resolve_prompt(prompt, template_name, file, &var_map, find_template)?;

    let agent = agent.or(agent_override).unwrap_or_else(|| "claude".into());

    // --command flag takes precedence, then template command
    let resolved_command = command.or(command_override);

    // Terminal agents with a command don't require a prompt
    let final_prompt = if resolved_prompt.is_none() && resolved_command.is_some() {
        String::new()
    } else {
        resolved_prompt.unwrap_or_default()
    };

    // Split --agent-args string into individual args (supports quoted values)
    let extra_args: Vec<String> = match agent_args {
        Some(s) => shell_words::split(&s)
            .map_err(|e| CliError::Other(format!("failed to parse --agent-args: {e}")))?,
        None => vec![],
    };

    let resp = client::send_request(
        socket,
        &Request::Spawn {
            project_root,
            prompt: final_prompt,
            agent,
            name,
            base,
            root,
            worktree,
            command: resolved_command,
            no_auto,
            extra_args,
            plan_mode,
            no_trigger,
            trigger,
            placement: None,
        },
    )
    .await?;
    let resp = output::check_response(resp, json)?;
    output::print_response(&resp, json)?;
    Ok(())
}

async fn fetch_template(
    socket: &Path,
    project_root: &str,
    name: &str,
) -> Result<template::Template, CliError> {
    let response = client::send_request(
        socket,
        &Request::GetTemplate {
            project_root: project_root.to_string(),
            name: name.to_string(),
        },
    )
    .await?;
    match output::check_response(response, false)? {
        Response::TemplateDetail {
            name,
            description,
            agent,
            body,
            source,
            command,
            ..
        } => Ok(template::Template {
            name,
            description,
            agent,
            body,
            source,
            command,
        }),
        other => Err(CliError::Other(format!(
            "unexpected response to template request: {other:?}"
        ))),
    }
}

/// Resolve the prompt from one of: --template, --file, or inline positional arg.
/// Returns (optional_prompt, optional_agent_override, optional_command_override).
#[allow(clippy::type_complexity)]
fn resolve_prompt(
    inline: Option<String>,
    template_name: Option<String>,
    file: Option<String>,
    vars: &HashMap<String, String>,
    find_template: impl FnOnce(&str) -> Option<template::Template>,
) -> Result<(Option<String>, Option<String>, Option<String>), CliError> {
    match (inline, template_name, file) {
        (Some(prompt), None, None) => Ok((Some(prompt), None, None)),
        (None, Some(name), None) => {
            let tpl = find_template(&name)
                .ok_or_else(|| CliError::Other(format!("template not found: {name}")))?;
            let agent_override = Some(tpl.agent.clone());
            let rendered = template::render(&tpl, vars);
            let command_override = template::render_command(&tpl, vars);
            Ok((Some(rendered), agent_override, command_override))
        }
        (None, None, Some(path)) => {
            let content = std::fs::read_to_string(&path)
                .map_err(|e| CliError::Other(format!("failed to read prompt file {path}: {e}")))?;
            // Parse as template (may have frontmatter)
            let file_name = Path::new(&path)
                .file_name()
                .map(|f| f.to_string_lossy().to_string())
                .unwrap_or_else(|| "prompt.md".into());
            let tpl = template::parse_template(&content, &file_name);
            let agent_override = if tpl.agent != "claude" {
                Some(tpl.agent.clone())
            } else {
                None
            };
            let rendered = template::render(&tpl, vars);
            let command_override = template::render_command(&tpl, vars);
            Ok((Some(rendered), agent_override, command_override))
        }
        // No prompt source — allowed when --command is set (terminal agent)
        (None, None, None) => Ok((None, None, None)),
        _ => Err(CliError::Other(
            "provide only one of: inline prompt, --template, or --file".into(),
        )),
    }
}

#[cfg(test)]
mod tests {}
