use std::path::Path;

use crate::client;
use crate::commands;
use crate::daemon_ctrl;
use crate::error::CliError;
use crate::output;
use pu_core::protocol::Request;

/// Parse comma-separated tags string into a Vec<String>.
/// Empty string returns empty vec. Whitespace around each tag is trimmed.
fn parse_tags(tags: &str) -> Vec<String> {
    if tags.is_empty() {
        return Vec::new();
    }
    tags.split(',')
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .collect()
}

pub async fn run_list(socket: &Path, json: bool) -> Result<(), CliError> {
    daemon_ctrl::ensure_daemon(socket).await?;
    let project_root = commands::cwd_string()?;
    let resp = client::send_request(
        socket,
        &Request::ListAgentDefs { project_root },
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
    agent_type: &str,
    template: Option<String>,
    inline_prompt: Option<String>,
    tags: &str,
    scope: &str,
    json: bool,
) -> Result<(), CliError> {
    daemon_ctrl::ensure_daemon(socket).await?;
    let project_root = commands::cwd_string()?;
    let tags_vec = parse_tags(tags);
    let resp = client::send_request(
        socket,
        &Request::SaveAgentDef {
            project_root,
            name: name.to_string(),
            agent_type: agent_type.to_string(),
            template,
            inline_prompt,
            tags: tags_vec,
            scope: scope.to_string(),
            available_in_command_dialog: true,
            icon: None,
        },
    )
    .await?;
    let resp = output::check_response(resp, json)?;
    output::print_response(&resp, json);
    Ok(())
}

pub async fn run_show(socket: &Path, name: &str, json: bool) -> Result<(), CliError> {
    daemon_ctrl::ensure_daemon(socket).await?;
    let project_root = commands::cwd_string()?;
    let resp = client::send_request(
        socket,
        &Request::GetAgentDef {
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
        &Request::DeleteAgentDef {
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

#[cfg(test)]
mod tests {
    use super::*;

    // --- parse_tags ---

    #[test]
    fn given_empty_string_should_return_empty_vec() {
        let result = parse_tags("");
        assert!(result.is_empty());
    }

    #[test]
    fn given_single_tag_should_return_one_element() {
        let result = parse_tags("review");
        assert_eq!(result, vec!["review"]);
    }

    #[test]
    fn given_comma_separated_tags_should_split() {
        let result = parse_tags("review,test,deploy");
        assert_eq!(result, vec!["review", "test", "deploy"]);
    }

    #[test]
    fn given_tags_with_whitespace_should_trim() {
        let result = parse_tags(" review , test , deploy ");
        assert_eq!(result, vec!["review", "test", "deploy"]);
    }

    #[test]
    fn given_trailing_comma_should_ignore_empty_entries() {
        let result = parse_tags("review,test,");
        assert_eq!(result, vec!["review", "test"]);
    }

    #[test]
    fn given_only_commas_should_return_empty_vec() {
        let result = parse_tags(",,,");
        assert!(result.is_empty());
    }
}
