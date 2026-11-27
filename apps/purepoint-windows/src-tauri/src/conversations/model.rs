//! Port of `Models/Conversation.swift` plus the `ChatMessage` / `ContentBlock` shapes the
//! transcript parser produces (`Models/ChatMessage.swift`, whose front-end port is WIN-8.16).
//!
//! Dates cross the IPC boundary as Unix milliseconds; an unknown modification date is
//! [`DISTANT_PAST_MS`](super::util::DISTANT_PAST_MS), as the Mac's `Date.distantPast`.

use serde::{Deserialize, Serialize, Serializer};

use super::util::last_path_component;

/// Which agent wrote a conversation (`AgentSource`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentSource {
    Claude,
    Codex,
    Opencode,
}

impl AgentSource {
    /// The Mac's `rawValue`.
    pub fn raw_value(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
            Self::Opencode => "opencode",
        }
    }
}

/// One agent session in a history index (`Conversation`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conversation {
    pub session_id: String,
    pub agent_source: AgentSource,
    pub title: String,
    pub preview_snippets: Vec<String>,
    pub project_path: Option<String>,
    pub pure_point_project_root: Option<String>,
    pub git_branch: Option<String>,
    pub transcript_path: Option<String>,
    pub created_at: Option<i64>,
    pub modified_at: i64,
    pub message_count: Option<i64>,
}

impl Conversation {
    /// `"<source>:<sessionId>"`, unique across sources.
    pub fn id(&self) -> String {
        format!("{}:{}", self.agent_source.raw_value(), self.session_id)
    }

    /// The PurePoint project's folder name, else the session's, else the home folder's.
    pub fn project_name(&self) -> String {
        let reference = self
            .pure_point_project_root
            .clone()
            .or_else(|| self.project_path.clone())
            .or_else(|| {
                std::env::var("USERPROFILE")
                    .or_else(|_| std::env::var("HOME"))
                    .ok()
            })
            .unwrap_or_default();
        last_path_component(&reference)
    }

    /// The session directory's folder name, or `~` when the session has none.
    pub fn workspace_name(&self) -> String {
        match &self.project_path {
            Some(path) => last_path_component(path),
            None => "~".to_string(),
        }
    }

    /// `withSnippets(_:)`.
    pub fn with_snippets(&self, snippets: Vec<String>) -> Self {
        Self {
            preview_snippets: snippets,
            ..self.clone()
        }
    }
}

/// The wire shape of a [`Conversation`]: its fields plus the computed `id`, `projectName`
/// and `workspaceName`, so the front end does not re-derive them.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ConversationWire<'a> {
    id: String,
    session_id: &'a str,
    agent_source: AgentSource,
    title: &'a str,
    preview_snippets: &'a [String],
    project_path: Option<&'a str>,
    pure_point_project_root: Option<&'a str>,
    git_branch: Option<&'a str>,
    transcript_path: Option<&'a str>,
    created_at: Option<i64>,
    modified_at: i64,
    message_count: Option<i64>,
    project_name: String,
    workspace_name: String,
}

impl Serialize for Conversation {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        ConversationWire {
            id: self.id(),
            session_id: &self.session_id,
            agent_source: self.agent_source,
            title: &self.title,
            preview_snippets: &self.preview_snippets,
            project_path: self.project_path.as_deref(),
            pure_point_project_root: self.pure_point_project_root.as_deref(),
            git_branch: self.git_branch.as_deref(),
            transcript_path: self.transcript_path.as_deref(),
            created_at: self.created_at,
            modified_at: self.modified_at,
            message_count: self.message_count,
            project_name: self.project_name(),
            workspace_name: self.workspace_name(),
        }
        .serialize(serializer)
    }
}

/// `ChatRole`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ChatRole {
    User,
    Assistant,
}

/// `ToolUseStatus`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ToolUseStatus {
    Running,
    Completed,
    Failed,
}

/// `ContentBlock`, minus the Point Guard-only `pulse` case (never in a transcript).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ContentBlock {
    Text {
        id: String,
        text: String,
    },
    CodeBlock {
        id: String,
        language: Option<String>,
        code: String,
    },
    ToolUse {
        id: String,
        name: String,
        input: String,
        status: ToolUseStatus,
    },
    ToolResult {
        id: String,
        tool_use_id: String,
        output: String,
        is_error: bool,
    },
}

/// `ChatMessage`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatMessage {
    pub id: String,
    pub role: ChatRole,
    /// Unix milliseconds.
    pub timestamp: i64,
    pub content_blocks: Vec<ContentBlock>,
    pub is_streaming: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conversation() -> Conversation {
        Conversation {
            session_id: "sid".into(),
            agent_source: AgentSource::Codex,
            title: "t".into(),
            preview_snippets: vec![],
            project_path: Some(r"C:\Users\me\repo\.pu\worktrees\wt-1".into()),
            pure_point_project_root: Some(r"C:\Users\me\repo".into()),
            git_branch: None,
            transcript_path: None,
            created_at: None,
            modified_at: 5,
            message_count: None,
        }
    }

    #[test]
    fn computed_names_and_id() {
        let c = conversation();
        assert_eq!(c.id(), "codex:sid");
        assert_eq!(c.project_name(), "repo");
        assert_eq!(c.workspace_name(), "wt-1");
        let bare = Conversation {
            project_path: None,
            ..c.clone()
        };
        assert_eq!(bare.workspace_name(), "~");
        assert_eq!(c.with_snippets(vec!["s".into()]).preview_snippets, ["s"]);
    }

    #[test]
    fn serializes_camel_case_with_computed_fields() {
        let json = serde_json::to_value(conversation()).unwrap();
        assert_eq!(json["id"], "codex:sid");
        assert_eq!(json["agentSource"], "codex");
        assert_eq!(json["purePointProjectRoot"], r"C:\Users\me\repo");
        assert_eq!(json["projectName"], "repo");
        assert_eq!(json["modifiedAt"], 5);
        assert!(json["gitBranch"].is_null());

        let block = ContentBlock::ToolResult {
            id: "b".into(),
            tool_use_id: "t".into(),
            output: "o".into(),
            is_error: true,
        };
        let json = serde_json::to_value(block).unwrap();
        assert_eq!(json["type"], "toolResult");
        assert_eq!(json["toolUseId"], "t");
        assert_eq!(json["isError"], true);
    }
}
