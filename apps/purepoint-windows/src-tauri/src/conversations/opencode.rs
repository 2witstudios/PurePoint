//! Port of `Services/OpenCodeConversationIndex.swift`: OpenCode sessions, one JSON file each,
//! under `<opencode data>/storage/session/<project hash>/*.json` (or directly in
//! `storage/session/`), shaped `{id, title, directory, time: {created, updated}, ...}`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde_json::Value;

use super::model::{AgentSource, Conversation};
use super::util::{
    DISTANT_PAST_MS, Object, is_hidden, locate_pure_point_project_root, obj_field, parse_timestamp,
    str_field,
};

/// Every readable session file under `base` (the `storage/session` directory). Unreadable or
/// id-less files are skipped; a missing directory lists nothing.
pub fn load_sessions(base: &Path) -> Vec<Conversation> {
    let Ok(top_level) = std::fs::read_dir(base) else {
        return vec![];
    };
    let mut top_level: Vec<PathBuf> = top_level
        .filter_map(Result::ok)
        .filter(|e| !is_hidden(&e.file_name().to_string_lossy()))
        .map(|e| e.path())
        .collect();
    top_level.sort();

    let mut cache = HashMap::new();
    let mut conversations = Vec::new();
    for entry in top_level {
        let targets: Vec<PathBuf> = if entry.is_dir() {
            let Ok(children) = std::fs::read_dir(&entry) else {
                continue;
            };
            let mut children: Vec<PathBuf> = children
                .filter_map(Result::ok)
                .filter(|e| !is_hidden(&e.file_name().to_string_lossy()))
                .map(|e| e.path())
                .filter(|p| is_json(p))
                .collect();
            children.sort();
            children
        } else if is_json(&entry) {
            vec![entry]
        } else {
            continue;
        };
        conversations.extend(
            targets
                .iter()
                .filter_map(|path| parse_session_file(path, &mut cache)),
        );
    }
    conversations
}

fn is_json(path: &Path) -> bool {
    path.extension().is_some_and(|ext| ext == "json")
}

fn parse_session_file(
    path: &Path,
    cache: &mut HashMap<String, Option<String>>,
) -> Option<Conversation> {
    let json: Object = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
    let id = str_field(&json, "id")?;
    let directory = str_field(&json, "directory").map(str::to_string);
    let time = obj_field(&json, "time");
    let time_field = |key: &str| time.and_then(|t| parse_flexible_timestamp(t.get(key)));
    Some(Conversation {
        session_id: id.to_string(),
        agent_source: AgentSource::Opencode,
        title: str_field(&json, "title")
            .unwrap_or("OpenCode session")
            .to_string(),
        preview_snippets: vec![],
        pure_point_project_root: directory
            .as_deref()
            .and_then(|dir| locate_pure_point_project_root(dir, cache)),
        project_path: directory,
        git_branch: None,
        transcript_path: None,
        created_at: time_field("created"),
        modified_at: time_field("updated").unwrap_or(DISTANT_PAST_MS),
        message_count: None,
    })
}

/// An OpenCode time: ISO 8601 text, or a Unix time in seconds or milliseconds, as a string or
/// a number. (The Mac reads only strings; OpenCode writes millisecond numbers.)
fn parse_flexible_timestamp(value: Option<&Value>) -> Option<i64> {
    let number = match value? {
        Value::String(text) if text.is_empty() => return None,
        Value::String(text) => {
            if let Some(ms) = parse_timestamp(Some(text)) {
                return Some(ms);
            }
            text.trim().parse::<f64>().ok()?
        }
        Value::Number(number) => number.as_f64()?,
        _ => return None,
    };
    if number > 1e12 {
        Some(number as i64)
    } else if number > 1e9 {
        Some((number * 1000.0) as i64)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::conversations::test_support::TempDir;
    use serde_json::json;

    #[test]
    fn loads_project_and_top_level_session_files() {
        let temp = TempDir::new();
        let project = temp.path().join("repo");
        std::fs::create_dir_all(project.join(".pu")).unwrap();
        std::fs::write(project.join(".pu").join("manifest.json"), "{}").unwrap();
        let directory = project.to_string_lossy().into_owned();
        temp.write(
            "session/proj-hash/ses_a.json",
            &json!({
                "id": "ses_a", "title": "Port the index", "directory": directory,
                "time": {"created": 1_764_100_000_000_i64, "updated": "1764100500"},
                "slug": "port", "projectID": "proj-hash",
            })
            .to_string(),
        );
        temp.write(
            "session/ses_b.json",
            &json!({"id": "ses_b", "time": {"updated": "2026-11-25T20:00:00Z"}}).to_string(),
        );
        temp.write("session/proj-hash/broken.json", "{");
        temp.write("session/proj-hash/no-id.json", "{}");
        temp.write("session/proj-hash/notes.txt", "x");

        let sessions = load_sessions(&temp.path().join("session"));

        assert_eq!(sessions.len(), 2, "{sessions:?}");
        let a = sessions.iter().find(|s| s.session_id == "ses_a").unwrap();
        assert_eq!(a.title, "Port the index");
        assert_eq!(a.project_path.as_deref(), Some(directory.as_str()));
        assert_eq!(
            a.pure_point_project_root.as_deref(),
            Some(directory.as_str())
        );
        assert_eq!(a.created_at, Some(1_764_100_000_000));
        assert_eq!(a.modified_at, 1_764_100_500_000);
        let b = sessions.iter().find(|s| s.session_id == "ses_b").unwrap();
        assert_eq!(b.title, "OpenCode session");
        assert_eq!(b.workspace_name(), "~");
        assert_eq!(b.created_at, None);
    }

    #[test]
    fn missing_storage_lists_nothing() {
        let temp = TempDir::new();
        assert!(load_sessions(&temp.path().join("storage/session")).is_empty());
    }

    #[test]
    fn small_numbers_are_not_times() {
        assert_eq!(parse_flexible_timestamp(Some(&json!(42))), None);
        assert_eq!(parse_flexible_timestamp(Some(&json!(""))), None);
        assert_eq!(parse_flexible_timestamp(Some(&json!(null))), None);
    }
}
