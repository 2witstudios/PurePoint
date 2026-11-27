//! Port of `Services/ClaudeConversationIndex.swift`: Claude Code sessions under
//! `<claude config>/projects/<encoded project dir>/`, in two phases.
//!
//! 1. [`load_indexed_sessions`] — fast: only each project's `sessions-index.json`.
//! 2. [`load_loose_sessions`] — slow: every top-level `<session id>.jsonl` not already
//!    indexed, reading a 24 KB head (and a 32 KB tail when the head lacks cwd, branch or
//!    session id) for metadata.
//!
//! Snippets are deferred to [`recent_snippets`] (a 64 KB tail read) for lazy enrichment.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::Value;

use super::model::{AgentSource, Conversation};
use super::util::{
    DISTANT_PAST_MS, Object, compact, file_mtime_ms, is_hidden, last_path_component,
    locate_pure_point_project_root, obj_field, parse_json_line, parse_timestamp, read_prefix,
    read_suffix, str_field, trimmed_non_empty,
};

type RootCache = HashMap<String, Option<String>>;

/// Claude Code's project-directory name for a working directory: every character that is
/// not an ASCII letter or digit becomes `-`. `C:\Users\me\dev` → `C--Users-me-dev`, and
/// `/Users/me/dev` → `-Users-me-dev`. The mapping is lossy, so the index never decodes it;
/// sessions carry their real `cwd`.
pub fn encode_project_dir(path: &str) -> String {
    path.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}

/// Whether a project directory holds throwaway temp-dir sessions: the Mac's
/// `private-var-folders` / `private-tmp`, plus the Windows `%TEMP%`
/// (`C:\Users\me\AppData\Local\Temp\...` encodes to `...-AppData-Local-Temp...`).
fn is_temp_project_dir(name: &str) -> bool {
    name.contains("private-var-folders")
        || name.contains("private-tmp")
        || name.to_ascii_lowercase().contains("-appdata-local-temp")
}

/// All sessions, indexed then loose, newest first, without snippets.
pub fn load_sessions(base: &Path, limit: Option<usize>) -> std::io::Result<Vec<Conversation>> {
    let mut conversations = load_indexed_sessions(base)?;
    let excluded: HashSet<String> = conversations.iter().map(|c| c.session_id.clone()).collect();
    conversations.extend(load_loose_sessions(base, &excluded)?);
    conversations.sort_by(|a, b| b.modified_at.cmp(&a.modified_at));
    if let Some(limit) = limit {
        conversations.truncate(limit);
    }
    Ok(conversations)
}

/// Phase 1: the sessions listed in `sessions-index.json` files (no JSONL reads).
pub fn load_indexed_sessions(base: &Path) -> std::io::Result<Vec<Conversation>> {
    let mut conversations = Vec::new();
    let mut seen = HashSet::new();
    let mut cache = RootCache::new();
    for directory in project_directories(base)? {
        for session in indexed_sessions_in(&directory, &mut cache) {
            if seen.insert(session.session_id.clone()) {
                conversations.push(session);
            }
        }
    }
    Ok(conversations)
}

/// Phase 2: sessions found by scanning `.jsonl` transcripts, skipping `excluded` ids.
pub fn load_loose_sessions(
    base: &Path,
    excluded: &HashSet<String>,
) -> std::io::Result<Vec<Conversation>> {
    let mut conversations = Vec::new();
    let mut seen = excluded.clone();
    let mut cache = RootCache::new();
    for directory in project_directories(base)? {
        for transcript in transcript_files(&directory)? {
            let Some(stem) = transcript
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
            else {
                continue;
            };
            if seen.insert(stem) {
                conversations.push(loose_session(&transcript, &mut cache));
            }
        }
    }
    Ok(conversations)
}

/// Up to `limit` recent user/assistant snippets from the transcript's last 64 KB, oldest first.
pub fn recent_snippets(transcript: &Path, limit: usize) -> Vec<String> {
    let Ok(tail) = read_suffix(transcript, 64 * 1024) else {
        return vec![];
    };
    let mut snippets: Vec<String> = Vec::new();
    for line in tail.split('\n').rev().filter(|l| !l.is_empty()) {
        if snippets.len() == limit {
            break;
        }
        let Some(snippet) = parse_json_line(line).and_then(|r| message_snippet(&r, 180)) else {
            continue;
        };
        if !snippets.contains(&snippet) {
            snippets.push(snippet);
        }
    }
    snippets.reverse();
    snippets
}

// MARK: - Directory discovery

fn project_directories(base: &Path) -> std::io::Result<Vec<PathBuf>> {
    if !base.exists() {
        return Ok(vec![]);
    }
    let mut directories: Vec<PathBuf> = std::fs::read_dir(base)?
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|t| t.is_dir()))
        .filter(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            !is_hidden(&name) && !is_temp_project_dir(&name)
        })
        .map(|entry| entry.path())
        .collect();
    directories.sort();
    Ok(directories)
}

/// A project directory's top-level transcripts, newest first. Subagent transcripts live in
/// `<session id>/subagents/` and are not sessions of their own.
fn transcript_files(directory: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut files: Vec<(i64, PathBuf)> = std::fs::read_dir(directory)?
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|t| t.is_file()))
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension().is_some_and(|ext| ext == "jsonl")
                && !path
                    .file_name()
                    .is_some_and(|n| is_hidden(&n.to_string_lossy()))
        })
        .map(|path| (file_mtime_ms(&path).unwrap_or(DISTANT_PAST_MS), path))
        .collect();
    files.sort_by(|a, b| b.0.cmp(&a.0));
    Ok(files.into_iter().map(|(_, path)| path).collect())
}

// MARK: - Indexed sessions

#[derive(Deserialize)]
struct SessionIndexFile {
    entries: Vec<SessionIndexEntry>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SessionIndexEntry {
    session_id: String,
    full_path: String,
    file_mtime: Option<i64>,
    first_prompt: Option<String>,
    summary: Option<String>,
    message_count: Option<i64>,
    created: Option<String>,
    modified: Option<String>,
    git_branch: Option<String>,
    project_path: Option<String>,
}

/// The entries of `directory/sessions-index.json` whose transcript exists. A missing or
/// undecodable index lists nothing, leaving its transcripts to the loose pass (the Mac throws
/// on an undecodable index, failing the whole phase).
fn indexed_sessions_in(directory: &Path, cache: &mut RootCache) -> Vec<Conversation> {
    let Ok(data) = std::fs::read(directory.join("sessions-index.json")) else {
        return vec![];
    };
    let Ok(index) = serde_json::from_slice::<SessionIndexFile>(&data) else {
        return vec![];
    };
    let directory_path = directory.to_string_lossy().into_owned();
    index
        .entries
        .into_iter()
        .filter(|entry| Path::new(&entry.full_path).exists())
        .map(|entry| {
            let transcript = Path::new(&entry.full_path);
            let project_path = entry
                .project_path
                .as_deref()
                .and_then(trimmed_non_empty)
                .map(str::to_string)
                .unwrap_or_else(|| directory_path.clone());
            let modified_at = parse_timestamp(entry.modified.as_deref())
                .or(entry.file_mtime)
                .or_else(|| file_mtime_ms(transcript))
                .unwrap_or(DISTANT_PAST_MS);
            Conversation {
                title: session_title(
                    entry.summary.as_deref(),
                    entry.first_prompt.as_deref(),
                    entry.git_branch.as_deref(),
                    &last_path_component(&project_path),
                ),
                session_id: entry.session_id,
                agent_source: AgentSource::Claude,
                preview_snippets: vec![],
                pure_point_project_root: locate_pure_point_project_root(&project_path, cache),
                project_path: Some(project_path),
                git_branch: entry
                    .git_branch
                    .as_deref()
                    .and_then(trimmed_non_empty)
                    .map(str::to_string),
                transcript_path: Some(entry.full_path.clone()),
                created_at: parse_timestamp(entry.created.as_deref()),
                modified_at,
                message_count: entry.message_count,
            }
        })
        .collect()
}

// MARK: - Loose sessions

#[derive(Default)]
struct TranscriptMetadata {
    session_id: Option<String>,
    project_path: Option<String>,
    git_branch: Option<String>,
    first_prompt: Option<String>,
    created_at: Option<i64>,
}

impl TranscriptMetadata {
    fn is_complete(&self) -> bool {
        self.project_path.is_some()
            && self.session_id.is_some()
            && self.first_prompt.is_some()
            && self.created_at.is_some()
    }

    fn needs_tail_scan(&self) -> bool {
        self.project_path.is_none() || self.git_branch.is_none() || self.session_id.is_none()
    }

    fn update(&mut self, record: &Object) {
        if self.session_id.is_none() {
            self.session_id = str_field(record, "sessionId").map(str::to_string);
        }
        if self.project_path.is_none() {
            self.project_path = str_field(record, "cwd").map(str::to_string);
        }
        if self.git_branch.is_none() {
            self.git_branch = str_field(record, "gitBranch").map(str::to_string);
        }
        if self.created_at.is_none() {
            self.created_at = parse_timestamp(str_field(record, "timestamp"));
        }
        if self.first_prompt.is_none() && str_field(record, "type") == Some("user") {
            self.first_prompt = message_snippet(record, 280);
        }
    }

    fn scan(&mut self, text: &str, until_complete: bool) {
        for line in text.split('\n').filter(|l| !l.is_empty()) {
            if let Some(record) = parse_json_line(line) {
                self.update(&record);
                if until_complete && self.is_complete() {
                    break;
                }
            }
        }
    }
}

fn transcript_metadata(transcript: &Path) -> TranscriptMetadata {
    let mut metadata = TranscriptMetadata::default();
    metadata.scan(
        &read_prefix(transcript, 24 * 1024).unwrap_or_default(),
        true,
    );
    if metadata.needs_tail_scan() {
        metadata.scan(
            &read_suffix(transcript, 32 * 1024).unwrap_or_default(),
            false,
        );
    }
    metadata
}

fn loose_session(transcript: &Path, cache: &mut RootCache) -> Conversation {
    let metadata = transcript_metadata(transcript);
    let session_id = metadata
        .session_id
        .as_deref()
        .and_then(trimmed_non_empty)
        .map(str::to_string)
        .unwrap_or_else(|| {
            transcript
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default()
        });
    let project_path = metadata
        .project_path
        .as_deref()
        .and_then(trimmed_non_empty)
        .map(str::to_string)
        .unwrap_or_else(|| {
            transcript
                .parent()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default()
        });
    Conversation {
        session_id,
        agent_source: AgentSource::Claude,
        title: session_title(
            None,
            metadata.first_prompt.as_deref(),
            metadata.git_branch.as_deref(),
            &last_path_component(&project_path),
        ),
        preview_snippets: vec![],
        pure_point_project_root: locate_pure_point_project_root(&project_path, cache),
        project_path: Some(project_path),
        git_branch: metadata
            .git_branch
            .as_deref()
            .and_then(trimmed_non_empty)
            .map(str::to_string),
        transcript_path: Some(transcript.to_string_lossy().into_owned()),
        created_at: metadata.created_at,
        modified_at: file_mtime_ms(transcript).unwrap_or(DISTANT_PAST_MS),
        message_count: None,
    }
}

// MARK: - Text extraction

fn message_snippet(record: &Object, max_length: usize) -> Option<String> {
    if !matches!(str_field(record, "type"), Some("user" | "assistant")) {
        return None;
    }
    normalized_text(obj_field(record, "message")?.get("content")?, max_length)
}

fn normalized_text(content: &Value, max_length: usize) -> Option<String> {
    match content {
        Value::String(text) => compact(Some(text), max_length),
        Value::Array(parts) => compact(Some(&text_from_content_parts(parts)), max_length),
        Value::Object(part) => compact(str_field(part, "text"), max_length),
        _ => None,
    }
}

fn text_from_content_parts(parts: &[Value]) -> String {
    parts
        .iter()
        .filter_map(Value::as_object)
        .filter_map(|part| match str_field(part, "type")? {
            "text" => str_field(part, "text"),
            "tool_result" => str_field(part, "content"),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join(" ")
}

// MARK: - Title resolution

fn session_title(
    summary: Option<&str>,
    first_prompt: Option<&str>,
    git_branch: Option<&str>,
    fallback: &str,
) -> String {
    title_from_summary(summary)
        .or_else(|| title_from_prompt(first_prompt))
        .or_else(|| title_from_branch(git_branch))
        .unwrap_or_else(|| fallback.to_string())
}

fn title_from_summary(summary: Option<&str>) -> Option<String> {
    let summary = trimmed_non_empty(summary?)?;
    let lower = summary.to_lowercase();
    if lower == "no prompt" || lower.contains("invalid api key") || lower.starts_with("error:") {
        return None;
    }
    Some(summary.to_string())
}

/// `text` without a leading ASCII `prefix`, compared case-insensitively.
fn strip_prefix_ignore_case<'a>(text: &'a str, prefix: &str) -> Option<&'a str> {
    let head = text.get(..prefix.len())?;
    head.eq_ignore_ascii_case(prefix)
        .then(|| &text[prefix.len()..])
}

const FILLER_PREFIXES: [&str; 6] = [
    "can you ",
    "could you ",
    "please ",
    "help me ",
    "i need to ",
    "i want to ",
];

fn title_from_prompt(prompt: Option<&str>) -> Option<String> {
    let prompt = trimmed_non_empty(prompt?)?;
    let cleaned = strip_prefix_ignore_case(prompt, "implement the following plan:")
        .map(str::trim_start)
        .unwrap_or(prompt);
    let cleaned = FILLER_PREFIXES
        .iter()
        .find_map(|filler| strip_prefix_ignore_case(cleaned, filler))
        .unwrap_or(cleaned);
    let first_line = cleaned
        .split(is_newline)
        .find(|line| !line.trim().is_empty())
        .unwrap_or(cleaned);
    let stripped = match first_line.strip_prefix('#') {
        Some(_) => first_line.trim_start_matches('#').trim_start(),
        None => first_line,
    };
    compact(Some(stripped), 72)
}

/// Swift's `Character.isNewline`.
fn is_newline(c: char) -> bool {
    matches!(
        c,
        '\n' | '\r' | '\u{0B}' | '\u{0C}' | '\u{85}' | '\u{2028}' | '\u{2029}'
    )
}

fn title_from_branch(branch: Option<&str>) -> Option<String> {
    let branch = trimmed_non_empty(branch?)?;
    let branch = ["pu/", "feature/", "fix/", "bugfix/", "hotfix/"]
        .iter()
        .find_map(|prefix| branch.strip_prefix(prefix))
        .unwrap_or(branch);
    let words = branch
        .replace(['-', '_'], " ")
        .split(' ')
        .filter(|word| !word.is_empty())
        .map(|word| {
            let mut chars = word.chars();
            let first = chars.next().map(|c| c.to_uppercase().collect::<String>());
            format!(
                "{}{}",
                first.unwrap_or_default(),
                chars.as_str().to_lowercase()
            )
        })
        .collect::<Vec<_>>()
        .join(" ");
    (!words.is_empty()).then_some(words)
}

/// Ports `ClaudeConversationIndexTests.swift` (one fn per Swift `@Test`), plus Windows cases.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::conversations::test_support::TempDir;
    use serde_json::json;

    enum Line<'a> {
        User(&'a str),
        Assistant(&'a str),
    }

    /// Swift's `writeTranscript`: a progress record then one record per line, a minute apart.
    fn write_transcript(path: &Path, cwd: &str, session_id: &str, branch: &str, lines: &[Line]) {
        let mut payloads = vec![json!({
            "type": "progress", "cwd": cwd, "sessionId": session_id, "gitBranch": branch,
            "timestamp": "2026-03-01T09:00:00.000Z",
        })];
        for (index, line) in lines.iter().enumerate() {
            let timestamp = format!("2026-03-01T09:{:02}:00.000Z", index + 1);
            let (kind, message) = match line {
                Line::User(content) => ("user", json!({"role": "user", "content": content})),
                Line::Assistant(content) => (
                    "assistant",
                    json!({"role": "assistant", "content": [{"type": "text", "text": content}]}),
                ),
            };
            payloads.push(json!({
                "type": kind, "cwd": cwd, "sessionId": session_id, "gitBranch": branch,
                "timestamp": timestamp, "message": message,
            }));
        }
        let text: Vec<String> = payloads.iter().map(Value::to_string).collect();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text.join("\n")).unwrap();
    }

    fn mkdir(path: &Path) {
        std::fs::create_dir_all(path).unwrap();
    }

    fn s(path: &Path) -> String {
        path.to_string_lossy().into_owned()
    }

    /// A PurePoint project (`.pu/manifest.json`) with one worktree; returns (root, worktree).
    fn pure_point_project(temp: &TempDir, name: &str, worktree: &str) -> (PathBuf, PathBuf) {
        let root = temp.path().join(name);
        let worktree = root.join(".pu").join("worktrees").join(worktree);
        mkdir(&worktree);
        std::fs::write(root.join(".pu").join("manifest.json"), "{}").unwrap();
        (root, worktree)
    }

    #[test]
    fn indexed_sessions_use_summary_and_resolve_pure_point_root() {
        let temp = TempDir::new();
        let (project_root, worktree) = pure_point_project(&temp, "purepoint", "wt-dashboard");
        let claude_base = temp.path().join("claude").join("projects");
        let transcript_dir = claude_base.join("project-hash");
        let transcript = transcript_dir.join("sid-indexed.jsonl");
        write_transcript(
            &transcript,
            &s(&worktree),
            "sid-indexed",
            "feature/dashboard",
            &[
                Line::User("Build a dashboard for session browsing"),
                Line::Assistant("I'll index the local transcript files and wire a dashboard."),
                Line::User("Show a pulse card too"),
                Line::Assistant("The pulse card is mocked, but the conversation browser is real."),
            ],
        );
        let index = json!({
            "version": 1,
            "entries": [{
                "sessionId": "sid-indexed",
                "fullPath": s(&transcript),
                "fileMtime": 1_770_046_959_218_i64,
                "firstPrompt": "Build a dashboard for session browsing",
                "summary": "Useful dashboard",
                "messageCount": 4,
                "created": "2026-03-01T09:00:00.000Z",
                "modified": "2026-03-01T09:10:05.000Z",
                "gitBranch": "feature/dashboard",
                "projectPath": s(&worktree),
            }],
        });
        std::fs::write(
            transcript_dir.join("sessions-index.json"),
            index.to_string(),
        )
        .unwrap();

        let sessions = load_sessions(&claude_base, None).unwrap();

        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].title, "Useful dashboard");
        assert_eq!(sessions[0].git_branch.as_deref(), Some("feature/dashboard"));
        assert_eq!(sessions[0].pure_point_project_root, Some(s(&project_root)));
        assert_eq!(sessions[0].message_count, Some(4));
        assert_eq!(
            sessions[0].modified_at,
            parse_timestamp(Some("2026-03-01T09:10:05.000Z")).unwrap()
        );
        // Snippets are deferred — load_sessions returns [] for lazy enrichment.
        assert!(sessions[0].preview_snippets.is_empty());
    }

    #[test]
    fn loose_sessions_fallback_to_transcript_metadata() {
        let temp = TempDir::new();
        let (project_root, worktree) = pure_point_project(&temp, "repo", "wt-raw");
        let claude_base = temp.path().join("claude").join("projects");
        write_transcript(
            &claude_base.join("project-hash-raw").join("sid-raw.jsonl"),
            &s(&worktree),
            "sid-raw",
            "feature/raw-browser",
            &[
                Line::User("Implement the following plan:\n\n# Resume old Claude session"),
                Line::Assistant(
                    "I'm reading the transcript directly because there is no sessions index here.",
                ),
                Line::User("Keep the conversation browser real"),
                Line::Assistant("Done. The dashboard now falls back to raw JSONL files."),
            ],
        );

        let sessions = load_sessions(&claude_base, None).unwrap();

        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].title, "Resume old Claude session");
        assert_eq!(
            sessions[0].git_branch.as_deref(),
            Some("feature/raw-browser")
        );
        assert_eq!(sessions[0].pure_point_project_root, Some(s(&project_root)));
        assert!(sessions[0].preview_snippets.is_empty());
    }

    #[test]
    fn recent_snippets_loads_from_transcript_tail() {
        let temp = TempDir::new();
        let transcript = temp.path().join("sid-snippets.jsonl");
        write_transcript(
            &transcript,
            "/tmp",
            "sid-snippets",
            "main",
            &[
                Line::User("First user message"),
                Line::Assistant("First assistant reply"),
                Line::User("Second user message"),
                Line::Assistant("Second assistant reply"),
            ],
        );

        assert_eq!(
            recent_snippets(&transcript, 3),
            [
                "First assistant reply",
                "Second user message",
                "Second assistant reply"
            ]
        );
    }

    #[test]
    fn temp_directories_are_skipped() {
        let temp = TempDir::new();
        let claude_base = temp.path().join("claude").join("projects");
        write_transcript(
            &claude_base
                .join("private-var-folders-xx-xxxxxx")
                .join("sid-temp.jsonl"),
            "/private/var/folders/xx/xxxxxx",
            "sid-temp",
            "main",
            &[Line::User("temp session")],
        );
        write_transcript(
            &claude_base.join("normal-project").join("sid-normal.jsonl"),
            "/tmp/normal",
            "sid-normal",
            "main",
            &[Line::User("normal session")],
        );

        let sessions = load_sessions(&claude_base, None).unwrap();

        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].session_id, "sid-normal");
    }

    #[test]
    fn bad_summaries_fallback_to_prompt_or_branch() {
        let temp = TempDir::new();
        let claude_base = temp.path().join("claude").join("projects");
        let dir = claude_base.join("project-bad");
        let transcript = dir.join("sid-bad.jsonl");
        write_transcript(
            &transcript,
            "/tmp",
            "sid-bad",
            "pu/simplify-engine",
            &[Line::User("  ")],
        );
        let index = json!({
            "version": 1,
            "entries": [{
                "sessionId": "sid-bad",
                "fullPath": s(&transcript),
                "summary": "error: invalid api key",
                "gitBranch": "pu/simplify-engine",
                "projectPath": "/tmp",
            }],
        });
        std::fs::write(dir.join("sessions-index.json"), index.to_string()).unwrap();

        let sessions = load_sessions(&claude_base, None).unwrap();

        assert_eq!(sessions.len(), 1);
        // Bad summary rejected, empty prompt, falls back to branch name.
        assert_eq!(sessions[0].title, "Simplify Engine");
    }

    #[test]
    fn filler_prefixes_are_stripped_from_prompt() {
        let temp = TempDir::new();
        let claude_base = temp.path().join("claude").join("projects");
        write_transcript(
            &claude_base.join("project-filler").join("sid-filler.jsonl"),
            "/tmp",
            "sid-filler",
            "main",
            &[Line::User("Can you fix the sidebar layout bug")],
        );

        let sessions = load_sessions(&claude_base, None).unwrap();

        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].title, "fix the sidebar layout bug");
    }

    // Windows cases beyond the Swift suite.

    /// The checked-in fixture `tests/fixtures/claude/projects/C--Users-dev-PurePoint/`, named
    /// the way Claude Code on Windows named this machine's project dirs
    /// (`C:\Users\dev\project` → `C--Users-dev-project`), with Windows `cwd`s, a non-message
    /// record first, and a `subagents/` transcript that must not become a session.
    #[test]
    fn windows_project_dir_fixture_loads_as_loose_session() {
        let base = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("claude")
            .join("projects");
        assert_eq!(
            encode_project_dir(r"C:\Users\dev\PurePoint"),
            "C--Users-dev-PurePoint"
        );

        let sessions = load_sessions(&base, None).unwrap();

        assert_eq!(sessions.len(), 1, "{sessions:?}");
        let session = &sessions[0];
        assert_eq!(session.session_id, "6f1c2d3e-0000-4000-8000-00000000c0de");
        assert_eq!(
            session.project_path.as_deref(),
            Some(r"C:\Users\dev\PurePoint")
        );
        assert_eq!(session.workspace_name(), "PurePoint");
        assert_eq!(session.git_branch.as_deref(), Some("pu/windows"));
        assert_eq!(session.title, "index Claude conversations on Windows");
        assert_eq!(
            session.created_at,
            parse_timestamp(Some("2026-11-25T20:06:36.221Z"))
        );
        let transcript = session.transcript_path.as_deref().unwrap();
        assert_eq!(
            recent_snippets(Path::new(transcript), 3),
            [
                "Please index Claude conversations on Windows",
                "Reading C:\\Users\\dev\\.claude\\projects now."
            ]
        );
    }

    #[test]
    fn encodes_project_dirs_like_claude_code() {
        assert_eq!(
            encode_project_dir(r"C:\Users\dev\project"),
            "C--Users-dev-project"
        );
        assert_eq!(
            encode_project_dir(r"C:\WINDOWS\system32"),
            "C--WINDOWS-system32"
        );
        assert_eq!(
            encode_project_dir(r"C:\Users\me\my repo\.pu\worktrees\wt-1"),
            "C--Users-me-my-repo--pu-worktrees-wt-1"
        );
        assert_eq!(encode_project_dir("/Users/me/dev"), "-Users-me-dev");
    }

    #[test]
    fn windows_temp_directories_are_skipped() {
        let temp = TempDir::new();
        let claude_base = temp.path().join("projects");
        write_transcript(
            &claude_base
                .join(encode_project_dir(
                    r"C:\Users\me\AppData\Local\Temp\pp-test",
                ))
                .join("sid-temp.jsonl"),
            r"C:\Users\me\AppData\Local\Temp\pp-test",
            "sid-temp",
            "main",
            &[Line::User("temp session")],
        );
        write_transcript(
            &claude_base.join("C--Users-me-repo").join("sid-repo.jsonl"),
            r"C:\Users\me\repo",
            "sid-repo",
            "main",
            &[Line::User("repo session")],
        );

        let ids: Vec<String> = load_sessions(&claude_base, None)
            .unwrap()
            .into_iter()
            .map(|c| c.session_id)
            .collect();
        assert_eq!(ids, ["sid-repo"]);
    }

    #[test]
    fn undecodable_index_leaves_transcripts_to_the_loose_pass() {
        let temp = TempDir::new();
        let claude_base = temp.path().join("projects");
        let dir = claude_base.join("C--repo");
        write_transcript(
            &dir.join("sid-a.jsonl"),
            r"C:\repo",
            "sid-a",
            "main",
            &[Line::User("hello")],
        );
        std::fs::write(dir.join("sessions-index.json"), "{not json").unwrap();

        assert!(load_indexed_sessions(&claude_base).unwrap().is_empty());
        let sessions = load_sessions(&claude_base, None).unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].title, "hello");
    }

    #[test]
    fn missing_base_lists_nothing() {
        let temp = TempDir::new();
        assert!(
            load_sessions(&temp.path().join("nope"), None)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn titles_follow_the_mac_rules() {
        assert_eq!(title_from_summary(Some("No Prompt")), None);
        assert_eq!(
            title_from_summary(Some("  Kept  ")).as_deref(),
            Some("Kept")
        );
        assert_eq!(
            title_from_prompt(Some("please ## Heading\nbody")).as_deref(),
            Some("Heading")
        );
        assert_eq!(
            title_from_branch(Some("feature/add_NEW-thing")).as_deref(),
            Some("Add New Thing")
        );
        assert_eq!(title_from_branch(Some("main")).as_deref(), Some("Main"));
    }
}
