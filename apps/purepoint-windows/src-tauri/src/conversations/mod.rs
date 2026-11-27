//! Agent conversation indexes (WIN-8.12): Claude, Codex and OpenCode session lists, the
//! Claude transcript parser and the stream-json event model, ported from the Mac's
//! `Services/{Claude,Codex,OpenCode}ConversationIndex.swift`, `Services/TranscriptParser.swift`
//! and `Models/{Conversation,StreamEvent}.swift`.
//!
//! # Where the histories live on Windows
//!
//! Observed on a Windows 11 machine with Claude Code 2.x, Codex CLI 0.17x and OpenCode 2.4
//! installed (2026-11-25):
//!
//! - **Claude**: `%USERPROFILE%\.claude\projects\<encoded cwd>\<session id>.jsonl`, or
//!   `%CLAUDE_CONFIG_DIR%\projects\...` when that is set. The project directory name is the
//!   session's working directory with every non-alphanumeric character replaced by `-`, so
//!   the drive colon and each backslash become dashes: `C:\Users\dev\project` →
//!   `C--Users-dev-project`, `C:\WINDOWS\system32` → `C--WINDOWS-system32` (see
//!   [`claude::encode_project_dir`]). Records carry the real path as `"cwd":"C:\\Users\\..."`
//!   and `gitBranch` (`"HEAD"` outside a branch), and lines end in LF. Each session may have a
//!   sibling `<session id>\` directory with `subagents\agent-*.jsonl` (+ `.meta.json`) and
//!   `tool-results\`; those are not sessions and the index ignores them, as on the Mac. No
//!   `sessions-index.json` existed on this machine, so phase 1 found nothing and every session
//!   came from the loose JSONL scan; the index format is the Mac's.
//! - **Codex**: `%CODEX_HOME%`, else `%USERPROFILE%\.codex`. The directory held `config.toml`,
//!   `auth.json` and SQLite state databases (`state_5.sqlite`, whose schema has a
//!   `rollout_path` column) but
//!   neither `session_index.jsonl` nor `sessions\` yet, since no session had been run. The
//!   index reads the Mac's file layout, `session_index.jsonl` plus
//!   `sessions\YYYY\MM\DD\rollout-<timestamp>-<uuid>.jsonl`, and lists nothing until they
//!   exist.
//! - **OpenCode**: `%XDG_DATA_HOME%\opencode`, else `%USERPROFILE%\.local\share\opencode`
//!   (OpenCode uses the XDG layout on Windows too, not `%APPDATA%`). It held only `log\` and
//!   `repos\`; `storage\session\` appears once a session is saved. The index reads the Mac's
//!   `storage\session\<project hash>\<session>.json` files.
//!
//! None of the three variables was set on the machine. Paths in records are native Windows
//! paths, so titles and names split on both `\` and `/`.

pub mod claude;
pub mod codex;
pub mod model;
pub mod opencode;
pub mod stream_event;
pub mod transcript;
pub mod util;

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::Deserialize;

pub use model::{AgentSource, ChatMessage, ChatRole, ContentBlock, Conversation, ToolUseStatus};
pub use stream_event::{StreamContentBlock, StreamEvent};

/// How many recent snippets a session row shows (`recentSnippets`' default).
pub const DEFAULT_SNIPPET_LIMIT: usize = 3;

/// An environment lookup, so path resolution is testable without touching the process env.
type Env<'a> = &'a dyn Fn(&str) -> Option<String>;

fn env_dir(env: Env, key: &str) -> Option<PathBuf> {
    env(key)
        .filter(|value| !value.trim().is_empty())
        .map(PathBuf::from)
}

fn home_dir(env: Env) -> PathBuf {
    env_dir(env, "USERPROFILE")
        .or_else(|| env_dir(env, "HOME"))
        .unwrap_or_default()
}

/// `CLAUDE_CONFIG_DIR\projects`, else `%USERPROFILE%\.claude\projects`.
pub fn claude_projects_dir(env: Env) -> PathBuf {
    env_dir(env, "CLAUDE_CONFIG_DIR")
        .unwrap_or_else(|| home_dir(env).join(".claude"))
        .join("projects")
}

/// `CODEX_HOME`, else `%USERPROFILE%\.codex`.
pub fn codex_home(env: Env) -> PathBuf {
    env_dir(env, "CODEX_HOME").unwrap_or_else(|| home_dir(env).join(".codex"))
}

/// `XDG_DATA_HOME\opencode\storage\session`, else
/// `%USERPROFILE%\.local\share\opencode\storage\session`.
pub fn opencode_session_dir(env: Env) -> PathBuf {
    env_dir(env, "XDG_DATA_HOME")
        .unwrap_or_else(|| home_dir(env).join(".local").join("share"))
        .join("opencode")
        .join("storage")
        .join("session")
}

fn process_env(key: &str) -> Option<String> {
    std::env::var(key).ok()
}

/// Which pass of a source's index to run (`SessionListState`'s phases on the Mac).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ListPhase {
    /// Fast: Claude `sessions-index.json` files, Codex `session_index.jsonl`, OpenCode files.
    Indexed,
    /// Slow: Claude loose transcripts not in `excluding`; Codex sessions enriched with their
    /// rollout's cwd and branch (replacing the indexed rows by id); nothing for OpenCode.
    Loose,
    /// Both, merged.
    #[default]
    All,
}

/// A path compared the way Windows does: `/` and `\` alike, no trailing separator, and
/// case-insensitive.
fn normalized_path(path: &str) -> String {
    let unified = path.replace('\\', "/");
    let trimmed = unified.trim_end_matches('/');
    let trimmed = if trimmed.is_empty() {
        &unified
    } else {
        trimmed
    };
    if cfg!(windows) {
        trimmed.to_lowercase()
    } else {
        trimmed.to_string()
    }
}

/// Whether a session belongs to the project at `filter`: its directory or PurePoint root is
/// the project or inside it (so worktree sessions count for their project).
pub fn matches_project(conversation: &Conversation, filter: &str) -> bool {
    let filter = normalized_path(filter);
    [
        conversation.project_path.as_deref(),
        conversation.pure_point_project_root.as_deref(),
    ]
    .into_iter()
    .flatten()
    .map(normalized_path)
    .any(|path| {
        path == filter
            || path
                .strip_prefix(&filter)
                .is_some_and(|rest| rest.starts_with('/') || filter.ends_with('/'))
    })
}

/// Lists one source's sessions for `phase`, newest first, optionally only those under
/// `project_path`. Codex rows have no directory until enriched, so a filtered Codex list
/// enriches every session.
pub fn list(
    source: AgentSource,
    phase: ListPhase,
    project_path: Option<&str>,
    excluding: &HashSet<String>,
    env: Env,
) -> std::io::Result<Vec<Conversation>> {
    let mut sessions = match source {
        AgentSource::Claude => {
            let base = claude_projects_dir(env);
            match phase {
                ListPhase::Indexed => claude::load_indexed_sessions(&base)?,
                ListPhase::Loose => claude::load_loose_sessions(&base, excluding)?,
                ListPhase::All => claude::load_sessions(&base, None)?,
            }
        }
        AgentSource::Codex => {
            let home = codex_home(env);
            let indexed = codex::load_indexed_sessions(&home)?;
            match phase {
                ListPhase::Indexed => indexed,
                ListPhase::Loose | ListPhase::All => {
                    let limit = if project_path.is_some() {
                        usize::MAX
                    } else {
                        codex::DEFAULT_ENRICH_LIMIT
                    };
                    codex::enrich_with_metadata(indexed, &home, limit)
                }
            }
        }
        AgentSource::Opencode => match phase {
            ListPhase::Loose => vec![],
            ListPhase::Indexed | ListPhase::All => {
                opencode::load_sessions(&opencode_session_dir(env))
            }
        },
    };
    if let Some(filter) = project_path {
        sessions.retain(|session| matches_project(session, filter));
    }
    sessions.sort_by(|a, b| b.modified_at.cmp(&a.modified_at));
    Ok(sessions)
}

async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|error| error.to_string())?
}

/// Lists a source's sessions (see [`list`]). `phase` defaults to `all`; `excluding` holds the
/// Claude session ids phase 1 already returned.
#[tauri::command]
pub async fn conversations_list(
    source: AgentSource,
    phase: Option<ListPhase>,
    project_path: Option<String>,
    excluding: Option<Vec<String>>,
) -> Result<Vec<Conversation>, String> {
    blocking(move || {
        let excluding: HashSet<String> = excluding.unwrap_or_default().into_iter().collect();
        list(
            source,
            phase.unwrap_or_default(),
            project_path.as_deref(),
            &excluding,
            &process_env,
        )
        .map_err(|error| error.to_string())
    })
    .await
}

/// Recent message snippets from a Claude transcript's tail, oldest first.
#[tauri::command]
pub async fn conversation_snippets(
    transcript_path: String,
    limit: Option<usize>,
) -> Result<Vec<String>, String> {
    blocking(move || {
        Ok(claude::recent_snippets(
            Path::new(&transcript_path),
            limit.unwrap_or(DEFAULT_SNIPPET_LIMIT),
        ))
    })
    .await
}

/// A Claude transcript as chat messages.
#[tauri::command]
pub async fn conversation_transcript(transcript_path: String) -> Result<Vec<ChatMessage>, String> {
    blocking(move || {
        transcript::parse(Path::new(&transcript_path)).map_err(|error| error.to_string())
    })
    .await
}

#[cfg(test)]
pub(crate) mod test_support {
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// A fresh directory under the system temp dir, removed on drop.
    pub struct TempDir(PathBuf);

    impl TempDir {
        pub fn new() -> Self {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let path = std::env::temp_dir().join(format!(
                "pp-conversations-{}-{}-{}",
                std::process::id(),
                super::util::now_ms(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        pub fn path(&self) -> &Path {
            &self.0
        }

        /// Writes `content` to `relative` (creating parents) and returns its path.
        pub fn write(&self, relative: &str, content: &str) -> PathBuf {
            let path = self.0.join(relative);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, content).unwrap();
            path
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::TempDir;
    use super::*;
    use std::collections::HashMap;

    fn env_of(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> + use<> {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        move |key| map.get(key).cloned()
    }

    #[test]
    fn resolves_default_locations_under_the_profile() {
        let env = env_of(&[("USERPROFILE", r"C:\Users\me"), ("HOME", "/ignored")]);
        let home = PathBuf::from(r"C:\Users\me");
        assert_eq!(
            claude_projects_dir(&env),
            home.join(".claude").join("projects")
        );
        assert_eq!(codex_home(&env), home.join(".codex"));
        assert_eq!(
            opencode_session_dir(&env),
            home.join(".local")
                .join("share")
                .join("opencode")
                .join("storage")
                .join("session")
        );
    }

    #[test]
    fn environment_overrides_win() {
        let env = env_of(&[
            ("USERPROFILE", r"C:\Users\me"),
            ("CLAUDE_CONFIG_DIR", r"D:\claude"),
            ("CODEX_HOME", r"D:\codex"),
            ("XDG_DATA_HOME", r"D:\data"),
        ]);
        assert_eq!(
            claude_projects_dir(&env),
            PathBuf::from(r"D:\claude").join("projects")
        );
        assert_eq!(codex_home(&env), PathBuf::from(r"D:\codex"));
        assert_eq!(
            opencode_session_dir(&env),
            PathBuf::from(r"D:\data")
                .join("opencode")
                .join("storage")
                .join("session")
        );
        let blank = env_of(&[("USERPROFILE", r"C:\Users\me"), ("CODEX_HOME", " ")]);
        assert_eq!(
            codex_home(&blank),
            PathBuf::from(r"C:\Users\me").join(".codex")
        );
    }

    fn conversation(project_path: Option<&str>, root: Option<&str>) -> Conversation {
        Conversation {
            session_id: "s".into(),
            agent_source: AgentSource::Claude,
            title: "t".into(),
            preview_snippets: vec![],
            project_path: project_path.map(str::to_string),
            pure_point_project_root: root.map(str::to_string),
            git_branch: None,
            transcript_path: None,
            created_at: None,
            modified_at: 0,
            message_count: None,
        }
    }

    #[test]
    fn project_filter_matches_the_project_and_its_worktrees() {
        let worktree = conversation(Some(r"C:\Users\me\repo\.pu\worktrees\wt-1"), None);
        assert!(matches_project(&worktree, r"C:\Users\me\repo"));
        assert!(matches_project(&worktree, "C:/Users/me/repo/"));
        assert!(!matches_project(&worktree, r"C:\Users\me\rep"));
        let rooted = conversation(Some(r"D:\elsewhere"), Some(r"C:\Users\me\repo"));
        assert!(matches_project(&rooted, r"C:\Users\me\repo"));
        assert!(!matches_project(
            &conversation(None, None),
            r"C:\Users\me\repo"
        ));
        if cfg!(windows) {
            assert!(matches_project(&worktree, r"c:\users\ME\Repo"));
        }
    }

    #[test]
    fn list_runs_each_claude_phase_and_filters_by_project() {
        let temp = TempDir::new();
        let claude_dir = temp.path().join("claude");
        let line = |cwd: &str, sid: &str| {
            serde_json::json!({
                "type": "user", "cwd": cwd, "sessionId": sid,
                "timestamp": "2026-11-25T20:00:00Z",
                "message": {"role": "user", "content": "hi"},
            })
            .to_string()
        };
        temp.write(
            "claude/projects/C--repo/sid-repo.jsonl",
            &line(r"C:\repo", "sid-repo"),
        );
        temp.write(
            "claude/projects/C--other/sid-other.jsonl",
            &line(r"C:\other", "sid-other"),
        );
        let claude_str = claude_dir.to_string_lossy().into_owned();
        let env = env_of(&[("CLAUDE_CONFIG_DIR", &claude_str)]);
        let none = HashSet::new();

        let all = list(AgentSource::Claude, ListPhase::All, None, &none, &env).unwrap();
        assert_eq!(all.len(), 2);
        let indexed = list(AgentSource::Claude, ListPhase::Indexed, None, &none, &env).unwrap();
        assert!(indexed.is_empty());
        let excluding: HashSet<String> = ["sid-other".to_string()].into();
        let loose = list(
            AgentSource::Claude,
            ListPhase::Loose,
            None,
            &excluding,
            &env,
        )
        .unwrap();
        assert_eq!(loose.len(), 1);
        assert_eq!(loose[0].session_id, "sid-repo");
        let filtered = list(
            AgentSource::Claude,
            ListPhase::All,
            Some(r"C:\repo"),
            &none,
            &env,
        )
        .unwrap();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].session_id, "sid-repo");

        let empty = env_of(&[("USERPROFILE", &claude_str)]);
        assert!(
            list(AgentSource::Codex, ListPhase::All, None, &none, &empty)
                .unwrap()
                .is_empty()
        );
        assert!(
            list(AgentSource::Opencode, ListPhase::All, None, &none, &empty)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn commands_serialize_to_the_front_end_shape() {
        let json = serde_json::to_value(conversation(Some(r"C:\repo"), None)).unwrap();
        let keys: Vec<&str> = json
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        for key in [
            "id",
            "sessionId",
            "agentSource",
            "title",
            "previewSnippets",
            "projectPath",
            "purePointProjectRoot",
            "gitBranch",
            "transcriptPath",
            "createdAt",
            "modifiedAt",
            "messageCount",
            "projectName",
            "workspaceName",
        ] {
            assert!(keys.contains(&key), "missing {key}");
        }
        let phase: ListPhase = serde_json::from_str(r#""loose""#).unwrap();
        assert_eq!(phase, ListPhase::Loose);
        let source: AgentSource = serde_json::from_str(r#""opencode""#).unwrap();
        assert_eq!(source, AgentSource::Opencode);
    }
}
