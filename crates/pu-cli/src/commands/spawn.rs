use crate::client;
use crate::commands::cwd_string;
use crate::daemon_ctrl;
use crate::error::CliError;
use crate::output;
use pu_core::protocol::Request;
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
    vars: Vec<String>,
    json: bool,
) -> Result<(), CliError> {
    daemon_ctrl::ensure_daemon(socket).await?;

    let cwd = std::env::current_dir()?;
    let project_root = cwd_string()?;

    // Parse --var KEY=VALUE pairs
    let var_map = parse_vars(&vars)?;

    // Resolve prompt + agent override from template/file/inline
    let (resolved_prompt, agent_override) =
        resolve_prompt(prompt, template_name, file, &var_map, &cwd)?;

    let agent = agent.or(agent_override).unwrap_or_else(|| "claude".into());

    let resp = client::send_request(
        socket,
        &Request::Spawn {
            project_root,
            prompt: resolved_prompt,
            agent,
            name,
            base,
            root,
            worktree,
        },
    )
    .await?;
    let resp = output::check_response(resp, json)?;
    output::print_response(&resp, json);
    Ok(())
}

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

/// Resolve the prompt from one of: --template, --file, or inline positional arg.
/// Returns (prompt_text, optional_agent_override).
fn resolve_prompt(
    inline: Option<String>,
    template_name: Option<String>,
    file: Option<String>,
    vars: &HashMap<String, String>,
    project_root: &Path,
) -> Result<(String, Option<String>), CliError> {
    match (inline, template_name, file) {
        (Some(prompt), None, None) => Ok((prompt, None)),
        (None, Some(name), None) => {
            let tpl = template::find_template(project_root, &name)
                .ok_or_else(|| CliError::Other(format!("template not found: {name}")))?;
            let agent_override = Some(tpl.agent.clone());
            let rendered = template::render(&tpl, vars);
            Ok((rendered, agent_override))
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
            Ok((rendered, agent_override))
        }
        (None, None, None) => Err(CliError::Other(
            "prompt required — provide inline, --template, or --file".into(),
        )),
        _ => Err(CliError::Other(
            "provide only one of: inline prompt, --template, or --file".into(),
        )),
    }
}
