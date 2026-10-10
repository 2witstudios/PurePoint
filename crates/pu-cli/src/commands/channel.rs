use crate::{client, error::CliError, output};
use clap::Subcommand;
use pu_core::protocol::{ChannelReference, Request, Response};
use std::path::{Path, PathBuf};

#[derive(Debug, Subcommand)]
pub enum ChannelAction {
    Send {
        text: String,
        #[arg(long)]
        reply_to: Option<String>,
        #[arg(long)]
        commit: Vec<String>,
        #[arg(long)]
        pr: Vec<String>,
        #[arg(long)]
        json: bool,
    },
    Read {
        #[arg(long, conflicts_with = "before")]
        since: Option<u64>,
        #[arg(long)]
        before: Option<u64>,
        #[arg(long, default_value = "100", value_parser = clap::value_parser!(u16).range(1..=200))]
        limit: u16,
        #[arg(long)]
        search: Option<String>,
        #[arg(long)]
        thread: Option<String>,
        #[arg(long)]
        json: bool,
    },
    Edit {
        id: String,
        text: String,
        #[arg(long)]
        json: bool,
    },
    React {
        id: String,
        #[arg(long)]
        remove: bool,
        #[arg(long)]
        json: bool,
    },
}

// Explicit and environment paths identify the project directly. Git discovery
// follows common metadata, so linked worktrees and root subdirectories agree.
fn resolve_root(
    explicit: Option<&str>,
    env_root: Option<&str>,
    cwd: &Path,
) -> Result<String, CliError> {
    if let Some(root) = explicit.or(env_root.filter(|r| !r.is_empty())) {
        return Ok(std::fs::canonicalize(root)?.to_string_lossy().into_owned());
    }
    let git = std::process::Command::new("git")
        .current_dir(cwd)
        .args(["rev-parse", "--path-format=absolute", "--git-common-dir"])
        .output()?;
    if !git.status.success() {
        return Err(CliError::Other(
            "cannot resolve project; use --project-root or PU_PROJECT_ROOT".into(),
        ));
    }
    let common = PathBuf::from(String::from_utf8_lossy(&git.stdout).trim());
    // The primary checkout can use a separate metadata directory. Its own
    // gitdir equals common-dir, so show-toplevel is safe for that checkout.
    let own = std::process::Command::new("git")
        .current_dir(cwd)
        .args(["rev-parse", "--absolute-git-dir"])
        .output()?;
    if own.status.success() && Path::new(String::from_utf8_lossy(&own.stdout).trim()) == common {
        let top = std::process::Command::new("git")
            .current_dir(cwd)
            .args(["rev-parse", "--show-toplevel"])
            .output()?;
        if top.status.success() {
            return Ok(std::fs::canonicalize(Path::new(
                String::from_utf8_lossy(&top.stdout).trim(),
            ))?
            .to_string_lossy()
            .into_owned());
        }
    }
    // Separate git dirs have no predictable parent: ask Git for the primary
    // worktree instead of treating the metadata directory as the project.
    let list = std::process::Command::new("git")
        .current_dir(cwd)
        .args(["worktree", "list", "--porcelain", "-z"])
        .output()?;
    if list.status.success()
        && let Some(root) = list
            .stdout
            .split(|b| *b == 0)
            .find_map(|field| field.strip_prefix(b"worktree "))
    {
        let candidate =
            std::fs::canonicalize(Path::new(&String::from_utf8_lossy(root).into_owned()))?;
        let manifest = pu_core::manifest::read_manifest(&candidate).map_err(|error| CliError::Other(format!("Git cannot identify an initialized primary project ({error}); use --project-root or PU_PROJECT_ROOT")))?;
        if std::fs::canonicalize(&manifest.project_root)? != candidate {
            return Err(CliError::Other("Git primary checkout does not match its project manifest; use --project-root or PU_PROJECT_ROOT".into()));
        }
        return Ok(candidate.to_string_lossy().into_owned());
    }
    Err(CliError::Other(
        "cannot resolve primary Git worktree; use --project-root".into(),
    ))
}

pub async fn run(
    socket: &Path,
    explicit: Option<String>,
    action: ChannelAction,
) -> Result<(), CliError> {
    let root_env = std::env::var("PU_PROJECT_ROOT").ok();
    let project_root = resolve_root(
        explicit.as_deref(),
        root_env.as_deref(),
        &std::env::current_dir()?,
    )?;
    let agent_id = std::env::var("PU_AGENT_ID").ok();
    run_with_context(socket, project_root, agent_id, action).await
}

async fn run_with_context(
    socket: &Path,
    project_root: String,
    agent_id: Option<String>,
    action: ChannelAction,
) -> Result<(), CliError> {
    let (request, json) = match action {
        ChannelAction::Send {
            text,
            reply_to,
            commit,
            pr,
            json,
        } => {
            let references = commit
                .into_iter()
                .map(|value| ChannelReference {
                    kind: "commit".into(),
                    value,
                    label: None,
                })
                .chain(pr.into_iter().map(|value| ChannelReference {
                    kind: "pr".into(),
                    value,
                    label: None,
                }))
                .collect();
            (
                Request::ChannelSend {
                    project_root,
                    agent_id,
                    text,
                    parent_id: reply_to,
                    references,
                },
                json,
            )
        }
        ChannelAction::Read {
            since,
            before,
            limit,
            search,
            thread,
            json,
        } => (
            Request::ChannelRead {
                project_root,
                agent_id,
                after: since,
                before,
                limit: limit.into(),
                query: search,
                parent_id: thread,
                known_revision: None,
            },
            json,
        ),
        ChannelAction::Edit { id, text, json } => (
            Request::ChannelEdit {
                project_root,
                agent_id,
                message_id: id,
                text,
            },
            json,
        ),
        ChannelAction::React { id, remove, json } => (
            Request::ChannelReact {
                project_root,
                agent_id,
                message_id: id,
                emoji: "👍".into(),
                active: !remove,
            },
            json,
        ),
    };
    // Posting is explicit and never starts an agent or injects terminal input.
    let response = output::check_response(client::send_request(socket, &request).await?, json)?;
    output::print_response(&response, json)
}

pub fn format_response(response: &Response) -> String {
    fn message(m: &pu_core::channel::ChannelMessage) -> String {
        let reply = m
            .parent_id
            .as_ref()
            .map(|id| format!(" reply-to={id}"))
            .unwrap_or_default();
        let edited = if m.edited_at.is_some() {
            " (edited)"
        } else {
            ""
        };
        let mut text = format!(
            "[{}] {} {} {}{}{}\n{}\n",
            m.sequence, m.id, m.author.name, m.created_at, reply, edited, m.text
        );
        for r in &m.references {
            text.push_str(&format!(
                "  {}: {}{}\n",
                r.kind,
                r.value,
                r.label
                    .as_ref()
                    .map(|s| format!(" ({s})"))
                    .unwrap_or_default()
            ));
        }
        for r in &m.reactions {
            text.push_str(&format!("  {} ×{}\n", r.emoji, r.author_ids.len()));
        }
        text
    }
    match response {
        Response::ChannelMessage { message: m, .. } => message(m),
        Response::ChannelHistory {
            messages,
            has_more,
            oldest_sequence,
            latest_sequence,
            ..
        } => {
            let mut text = messages.iter().map(message).collect::<String>();
            if messages.is_empty() {
                text.push_str("No channel messages.\n");
            }
            text.push_str(&format!("Latest sequence: {latest_sequence}\n"));
            if *has_more {
                text.push_str(&format!(
                    "More matching history available (oldest in this window: {}).\n",
                    oldest_sequence.unwrap_or(0)
                ));
            }
            text
        }
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests;
