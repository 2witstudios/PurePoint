//! Channel operations have no session, PTY, trigger or scheduler dependencies.
use pu_core::channel::{ChannelAuthor, ChannelError, ChannelStore, ReadOptions};
use pu_core::protocol::{Request, Response};
use pu_core::types::AgentLocation;
use std::path::Path;

fn identity(root: &Path, agent_id: Option<&str>) -> Result<ChannelAuthor, ChannelError> {
    // Even human reads require an initialized project. Do not create a channel
    // in an arbitrary directory or hide a corrupt manifest behind empty history.
    let manifest =
        pu_core::manifest::read_manifest(root).map_err(|e| ChannelError::Invalid(e.to_string()))?;
    if let Some(id) = agent_id {
        let location = manifest
            .find_agent(id)
            .ok_or_else(|| ChannelError::Invalid(format!("unknown project agent: {id}")))?;
        let (agent, worktree) = match location {
            AgentLocation::Root(agent) => (agent, None),
            AgentLocation::Worktree { worktree, agent } => (agent, Some(worktree)),
        };
        if agent.id != id {
            return Err(ChannelError::Invalid("manifest agent ID mismatch".into()));
        }
        return Ok(ChannelAuthor {
            id: id.into(),
            name: agent.name.clone(),
            kind: "agent".into(),
            agent_type: Some(agent.agent_type.clone()),
            worktree_id: worktree.map(|w| w.id.clone()),
            branch: worktree.map(|w| w.branch.clone()),
        });
    }
    // OS account identity, never a client-supplied name or environment username.
    // SAFETY: geteuid has no pointer arguments or preconditions.
    let uid = unsafe { libc::geteuid() };
    let name = std::process::Command::new("/usr/bin/id")
        .args(["-un", &uid.to_string()])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| format!("Developer {uid}"));
    Ok(ChannelAuthor {
        id: format!("human:{uid}"),
        name,
        kind: "human".into(),
        agent_type: None,
        worktree_id: None,
        branch: None,
    })
}

pub(super) async fn handle(request: Request) -> Response {
    match tokio::task::spawn_blocking(move || execute(request)).await {
        Ok(Ok(response)) => response,
        Ok(Err(error)) => Response::Error {
            code: match error {
                ChannelError::Invalid(_) => "CHANNEL_INVALID",
                ChannelError::Missing(_) => "CHANNEL_NOT_FOUND",
                ChannelError::Ownership => "CHANNEL_OWNERSHIP",
                ChannelError::Version(_) => "CHANNEL_VERSION",
                ChannelError::Corrupt(_) | ChannelError::Json(_) => "CHANNEL_CORRUPT",
                ChannelError::Io(_) => "CHANNEL_IO",
            }
            .into(),
            message: error.to_string(),
        },
        Err(error) => Response::Error {
            code: "CHANNEL_INTERNAL".into(),
            message: error.to_string(),
        },
    }
}
fn execute(request: Request) -> Result<Response, ChannelError> {
    let (root, agent_id) = match &request {
        Request::ChannelRead {
            project_root,
            agent_id,
            ..
        }
        | Request::ChannelSend {
            project_root,
            agent_id,
            ..
        }
        | Request::ChannelEdit {
            project_root,
            agent_id,
            ..
        }
        | Request::ChannelReact {
            project_root,
            agent_id,
            ..
        } => (Path::new(project_root), agent_id.as_deref()),
        _ => unreachable!("channel request dispatch"),
    };
    let author = identity(root, agent_id)?;
    let store = ChannelStore::new(root);
    let (message, revision) = match request {
        Request::ChannelRead {
            after,
            before,
            limit,
            query,
            parent_id,
            known_revision,
            ..
        } => {
            let history = store.read(&ReadOptions {
                after,
                before,
                limit,
                query,
                parent_id,
                known_revision,
            })?;
            return Ok(Response::ChannelHistory {
                messages: history.messages,
                revision: history.revision,
                latest_sequence: history.latest_sequence,
                has_more: history.has_more,
                oldest_sequence: history.oldest_sequence,
                unchanged: history.unchanged,
                self_author_id: author.id,
                reply_counts: history.reply_counts,
            });
        }
        Request::ChannelSend {
            text,
            parent_id,
            references,
            ..
        } => store.send(author, text, parent_id, references)?,
        Request::ChannelEdit {
            message_id, text, ..
        } => store.edit(&author.id, &message_id, text)?,
        Request::ChannelReact {
            message_id,
            emoji,
            active,
            ..
        } => store.react(&author.id, &message_id, &emoji, active)?,
        _ => unreachable!("channel request dispatch"),
    };
    Ok(Response::ChannelMessage { message, revision })
}

#[cfg(test)]
mod tests;
