//! Durable per-project channel. All reads and updates hold the same persistent
//! advisory lock; mutations fsync a unique temporary file before atomic replace.
use std::collections::{BTreeMap, HashSet};
use std::io::Write;
use std::path::{Path, PathBuf};

use fs4::FileExt;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChannelAuthor {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub agent_type: Option<String>,
    pub worktree_id: Option<String>,
    pub branch: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChannelReference {
    pub kind: String,
    pub value: String,
    pub label: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChannelReaction {
    pub emoji: String,
    pub author_ids: Vec<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChannelMessage {
    pub id: String,
    pub sequence: u64,
    pub parent_id: Option<String>,
    pub author: ChannelAuthor,
    pub text: String,
    pub created_at: String,
    pub edited_at: Option<String>,
    pub references: Vec<ChannelReference>,
    pub reactions: Vec<ChannelReaction>,
}
#[derive(Debug, thiserror::Error)]
pub enum ChannelError {
    #[error("{0}")]
    Invalid(String),
    #[error("channel message not found: {0}")]
    Missing(String),
    #[error("only the author may edit a message")]
    Ownership,
    #[error("unsupported channel store version: {0}")]
    Version(u32),
    #[error("corrupt channel store: {0}")]
    Corrupt(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}
#[derive(Debug, Clone)]
pub struct ReadOptions {
    pub after: Option<u64>,
    pub before: Option<u64>,
    pub limit: usize,
    pub query: Option<String>,
    pub parent_id: Option<String>,
    pub known_revision: Option<u64>,
}
impl Default for ReadOptions {
    fn default() -> Self {
        Self {
            after: None,
            before: None,
            limit: 100,
            query: None,
            parent_id: None,
            known_revision: None,
        }
    }
}
#[derive(Debug)]
pub struct ChannelHistory {
    pub messages: Vec<ChannelMessage>,
    pub revision: u64,
    pub latest_sequence: u64,
    pub has_more: bool,
    pub oldest_sequence: Option<u64>,
    pub unchanged: bool,
    pub reply_counts: BTreeMap<String, u64>,
}
#[derive(Debug, Default, Serialize, Deserialize)]
struct Data {
    version: u32,
    revision: u64,
    latest_sequence: u64,
    messages: Vec<ChannelMessage>,
}

pub struct ChannelStore {
    path: PathBuf,
}
impl ChannelStore {
    pub fn new(project_root: &Path) -> Self {
        Self {
            path: project_root.join(".pu/channel.json"),
        }
    }

    fn locked<T>(
        &self,
        action: impl FnOnce(&mut Data) -> Result<(T, bool), ChannelError>,
    ) -> Result<T, ChannelError> {
        let lock = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(self.path.with_extension("json.lock"))?;
        FileExt::lock(&lock)?;
        let mut data = match std::fs::read(&self.path) {
            Ok(bytes) => {
                #[derive(Deserialize)]
                struct Version {
                    version: u32,
                }
                let version = serde_json::from_slice::<Version>(&bytes)?.version;
                if version != 1 {
                    return Err(ChannelError::Version(version));
                }
                serde_json::from_slice::<Data>(&bytes)?
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Data {
                version: 1,
                ..Data::default()
            },
            Err(e) => return Err(e.into()),
        };
        if data.version != 1 {
            return Err(ChannelError::Version(data.version));
        }
        validate_data(&data)?;
        let (result, changed) = action(&mut data)?;
        if changed {
            self.persist(&data)?;
        }
        Ok(result)
    }

    fn persist(&self, data: &Data) -> Result<(), ChannelError> {
        let temp = self
            .path
            .with_extension(format!("json.tmp.{}", uuid::Uuid::new_v4()));
        let write = || -> Result<(), ChannelError> {
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temp)?;
            file.write_all(&serde_json::to_vec_pretty(data)?)?;
            file.write_all(b"\n")?;
            file.sync_all()?;
            std::fs::rename(&temp, &self.path)?;
            std::fs::File::open(self.path.parent().expect("channel parent"))?.sync_all()?;
            Ok(())
        };
        write().inspect_err(|_| {
            let _ = std::fs::remove_file(&temp);
        })
    }

    pub fn send(
        &self,
        author: ChannelAuthor,
        text: String,
        parent_id: Option<String>,
        references: Vec<ChannelReference>,
    ) -> Result<(ChannelMessage, u64), ChannelError> {
        validate_author(&author)?;
        validate_text(&text)?;
        validate_references(&references)?;
        self.locked(|data| {
            if let Some(parent) = &parent_id {
                require_parent(data, parent)?;
            }
            let sequence = next(data.latest_sequence)?;
            let revision = next(data.revision)?;
            let message = ChannelMessage {
                id: format!("msg-{}", uuid::Uuid::new_v4()),
                sequence,
                parent_id,
                author,
                text,
                created_at: chrono::Utc::now().to_rfc3339(),
                edited_at: None,
                references,
                reactions: vec![],
            };
            data.latest_sequence = sequence;
            data.revision = revision;
            data.messages.push(message.clone());
            Ok(((message, revision), true))
        })
    }

    pub fn edit(
        &self,
        author_id: &str,
        message_id: &str,
        text: String,
    ) -> Result<(ChannelMessage, u64), ChannelError> {
        validate_text(&text)?;
        self.locked(|data| {
            let revision = next(data.revision)?;
            let message = data
                .messages
                .iter_mut()
                .find(|m| m.id == message_id)
                .ok_or_else(|| ChannelError::Missing(message_id.into()))?;
            if message.author.id != author_id {
                return Err(ChannelError::Ownership);
            }
            message.text = text;
            message.edited_at = Some(chrono::Utc::now().to_rfc3339());
            data.revision = revision;
            Ok(((message.clone(), revision), true))
        })
    }

    pub fn react(
        &self,
        author_id: &str,
        message_id: &str,
        emoji: &str,
        active: bool,
    ) -> Result<(ChannelMessage, u64), ChannelError> {
        validate_emoji(emoji)?;
        if author_id.is_empty() {
            return Err(ChannelError::Invalid("empty reaction identity".into()));
        }
        self.locked(|data| {
            let message = data
                .messages
                .iter_mut()
                .find(|m| m.id == message_id)
                .ok_or_else(|| ChannelError::Missing(message_id.into()))?;
            let previous = message.reactions.clone();
            if let Some(reaction) = message.reactions.iter_mut().find(|r| r.emoji == emoji) {
                reaction.author_ids.retain(|id| id != author_id);
                if active {
                    reaction.author_ids.push(author_id.into());
                }
                reaction.author_ids.sort();
            } else if active {
                message.reactions.push(ChannelReaction {
                    emoji: emoji.into(),
                    author_ids: vec![author_id.into()],
                });
            }
            message.reactions.retain(|r| !r.author_ids.is_empty());
            let changed = previous != message.reactions;
            if changed {
                data.revision = next(data.revision)?;
            }
            Ok(((message.clone(), data.revision), changed))
        })
    }

    pub fn read(&self, options: &ReadOptions) -> Result<ChannelHistory, ChannelError> {
        if options.after.is_some() && options.before.is_some() {
            return Err(ChannelError::Invalid(
                "after and before are mutually exclusive".into(),
            ));
        }
        if !(1..=200).contains(&options.limit) {
            return Err(ChannelError::Invalid("read limit must be 1–200".into()));
        }
        if options
            .query
            .as_ref()
            .is_some_and(|q| q.chars().count() > 256)
        {
            return Err(ChannelError::Invalid("query exceeds 256 characters".into()));
        }
        self.locked(|data| {
            if let Some(parent) = &options.parent_id {
                require_parent(data, parent)?;
            }
            let unchanged = options.after.is_none()
                && options.before.is_none()
                && options.query.is_none()
                && options.parent_id.is_none()
                && options.known_revision == Some(data.revision);
            let mut history = ChannelHistory {
                messages: vec![],
                revision: data.revision,
                latest_sequence: data.latest_sequence,
                has_more: false,
                oldest_sequence: None,
                unchanged,
                reply_counts: BTreeMap::new(),
            };
            if unchanged {
                return Ok((history, false));
            }
            let query = options.query.as_ref().map(|q| q.to_lowercase());
            let mut matches: Vec<_> = data
                .messages
                .iter()
                .filter(|m| {
                    options.after.is_none_or(|s| m.sequence > s)
                        && options.before.is_none_or(|s| m.sequence < s)
                        && options
                            .parent_id
                            .as_ref()
                            .is_none_or(|p| m.parent_id.as_ref() == Some(p))
                        && query.as_ref().is_none_or(|q| searchable(m).contains(q))
                })
                .collect();
            history.has_more = matches.len() > options.limit;
            if options.after.is_some() {
                matches.truncate(options.limit);
            } else {
                matches = matches.into_iter().rev().take(options.limit).collect();
                matches.reverse();
            }
            history.oldest_sequence = matches.first().map(|m| m.sequence);
            let parents: HashSet<_> = matches
                .iter()
                .filter_map(|m| m.parent_id.as_ref())
                .collect();
            history.messages = matches.iter().map(|m| (*m).clone()).collect();
            for parent in parents {
                if !history.messages.iter().any(|m| &m.id == parent) {
                    history.messages.push(
                        data.messages
                            .iter()
                            .find(|m| &m.id == parent)
                            .expect("validated parent")
                            .clone(),
                    );
                }
            }
            history.messages.sort_by_key(|m| m.sequence);
            for parent in history.messages.iter().filter(|m| m.parent_id.is_none()) {
                history.reply_counts.insert(
                    parent.id.clone(),
                    data.messages
                        .iter()
                        .filter(|m| m.parent_id.as_ref() == Some(&parent.id))
                        .count() as u64,
                );
            }
            Ok((history, false))
        })
    }
}
fn next(value: u64) -> Result<u64, ChannelError> {
    value
        .checked_add(1)
        .ok_or_else(|| ChannelError::Corrupt("counter exhausted".into()))
}
fn require_parent(data: &Data, id: &str) -> Result<(), ChannelError> {
    if data
        .messages
        .iter()
        .any(|m| m.id == id && m.parent_id.is_none())
    {
        Ok(())
    } else {
        Err(ChannelError::Invalid(
            "parent must be an existing top-level message".into(),
        ))
    }
}
fn validate_text(text: &str) -> Result<(), ChannelError> {
    if text.trim().is_empty() || text.len() > 16_000 {
        Err(ChannelError::Invalid(
            "text must contain 1–16000 UTF-8 bytes and not be whitespace-only".into(),
        ))
    } else {
        Ok(())
    }
}
fn validate_author(author: &ChannelAuthor) -> Result<(), ChannelError> {
    if author.id.is_empty()
        || author.name.trim().is_empty()
        || !matches!(author.kind.as_str(), "human" | "agent")
    {
        Err(ChannelError::Invalid("invalid author".into()))
    } else {
        Ok(())
    }
}
fn validate_emoji(emoji: &str) -> Result<(), ChannelError> {
    if emoji.trim().is_empty() || emoji.len() > 64 || emoji.chars().any(char::is_control) {
        Err(ChannelError::Invalid(
            "emoji must contain 1–64 bytes without control characters".into(),
        ))
    } else {
        Ok(())
    }
}
fn validate_references(references: &[ChannelReference]) -> Result<(), ChannelError> {
    if references.len() > 8 {
        return Err(ChannelError::Invalid("at most 8 references".into()));
    }
    for reference in references {
        let valid = match reference.kind.as_str() {
            "commit" => {
                (7..=64).contains(&reference.value.len())
                    && reference.value.bytes().all(|c| c.is_ascii_hexdigit())
            }
            "pr" => {
                !reference.value.is_empty()
                    && reference.value.len() <= 20
                    && reference.value.bytes().all(|c| c.is_ascii_digit())
                    && reference.value.parse::<u64>().is_ok_and(|n| n > 0)
            }
            _ => false,
        };
        if !valid
            || reference
                .label
                .as_ref()
                .is_some_and(|s| s.len() > 256 || s.chars().any(char::is_control))
        {
            return Err(ChannelError::Invalid(
                "invalid commit/PR reference or label (max 256 bytes)".into(),
            ));
        }
    }
    Ok(())
}
fn searchable(m: &ChannelMessage) -> String {
    [
        m.text.clone(),
        m.author.name.clone(),
        m.author.branch.clone().unwrap_or_default(),
        m.author.worktree_id.clone().unwrap_or_default(),
        m.references
            .iter()
            .map(|r| format!("{} {}", r.value, r.label.as_deref().unwrap_or_default()))
            .collect::<Vec<_>>()
            .join(" "),
    ]
    .join(" ")
    .to_lowercase()
}
fn validate_data(data: &Data) -> Result<(), ChannelError> {
    let validate = || -> Result<(), ChannelError> {
        if data.latest_sequence != data.messages.len() as u64
            || data.revision < data.latest_sequence
        {
            return Err(ChannelError::Invalid("inconsistent counters".into()));
        }
        let mut ids = HashSet::new();
        for (i, m) in data.messages.iter().enumerate() {
            if m.sequence != i as u64 + 1 || m.id.is_empty() || !ids.insert(&m.id) {
                return Err(ChannelError::Invalid(
                    "duplicate ID or invalid sequence".into(),
                ));
            }
            validate_author(&m.author)?;
            validate_text(&m.text)?;
            validate_references(&m.references)?;
            chrono::DateTime::parse_from_rfc3339(&m.created_at)
                .map_err(|e| ChannelError::Invalid(e.to_string()))?;
            if let Some(time) = &m.edited_at {
                chrono::DateTime::parse_from_rfc3339(time)
                    .map_err(|e| ChannelError::Invalid(e.to_string()))?;
            }
            if let Some(parent) = &m.parent_id
                && !data.messages[..i]
                    .iter()
                    .any(|p| &p.id == parent && p.parent_id.is_none())
            {
                return Err(ChannelError::Invalid("invalid parent".into()));
            }
            let mut emojis = HashSet::new();
            for r in &m.reactions {
                validate_emoji(&r.emoji)?;
                let ids: HashSet<_> = r.author_ids.iter().collect();
                if !emojis.insert(&r.emoji)
                    || r.author_ids.is_empty()
                    || ids.len() != r.author_ids.len()
                    || ids.iter().any(|id| id.is_empty())
                {
                    return Err(ChannelError::Invalid("invalid reactions".into()));
                }
            }
        }
        Ok(())
    };
    validate().map_err(|e| ChannelError::Corrupt(e.to_string()))
}

#[cfg(test)]
mod tests;
