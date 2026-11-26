//! Port of `Services/CodexConversationIndex.swift`: Codex sessions from
//! `<codex home>/session_index.jsonl` (one `{id, thread_name, updated_at}` per line), then
//! enriched with `cwd` and git branch from the first line (`session_meta`) of each session's
//! rollout, `<codex home>/sessions/YYYY/MM/DD/rollout-<timestamp>-<uuid>.jsonl`.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use super::model::{AgentSource, Conversation};
use super::util::{
    DISTANT_PAST_MS, is_hidden, locate_pure_point_project_root, obj_field, parse_json_line,
    parse_timestamp, read_prefix, str_field,
};

/// How many sessions the Mac enriches by default.
pub const DEFAULT_ENRICH_LIMIT: usize = 50;

/// Phase 1: the sessions in `session_index.jsonl`, first entry per id. A missing index lists
/// nothing.
pub fn load_indexed_sessions(base: &Path) -> std::io::Result<Vec<Conversation>> {
    let index = base.join("session_index.jsonl");
    if !index.exists() {
        return Ok(vec![]);
    }
    let bytes = std::fs::read(index)?;
    let text = String::from_utf8_lossy(&bytes);
    let mut seen = HashSet::new();
    let mut conversations = Vec::new();
    for line in text.split('\n').filter(|l| !l.is_empty()) {
        let Some(record) = parse_json_line(line) else {
            continue;
        };
        let Some(id) = str_field(&record, "id") else {
            continue;
        };
        if !seen.insert(id.to_string()) {
            continue;
        }
        conversations.push(Conversation {
            session_id: id.to_string(),
            agent_source: AgentSource::Codex,
            title: str_field(&record, "thread_name")
                .unwrap_or("Codex session")
                .to_string(),
            preview_snippets: vec![],
            project_path: None,
            pure_point_project_root: None,
            git_branch: None,
            transcript_path: None,
            created_at: None,
            modified_at: parse_timestamp(str_field(&record, "updated_at"))
                .unwrap_or(DISTANT_PAST_MS),
            message_count: None,
        });
    }
    Ok(conversations)
}

/// Phase 2: the first `limit` sessions gain `cwd`, branch, PurePoint root and transcript path
/// from their rollout's first line; the rest pass through unchanged.
pub fn enrich_with_metadata(
    sessions: Vec<Conversation>,
    base: &Path,
    limit: usize,
) -> Vec<Conversation> {
    let files = session_file_paths(base);
    let mut cache = HashMap::new();
    sessions
        .into_iter()
        .enumerate()
        .map(|(index, session)| {
            if index >= limit {
                return session;
            }
            let Some(file) = files.get(&session.session_id) else {
                return session;
            };
            let Ok(head) = read_prefix(file, 8 * 1024) else {
                return session;
            };
            let first_line = head.split('\n').next().unwrap_or_default();
            let Some(record) = parse_json_line(first_line) else {
                return session;
            };
            let payload = obj_field(&record, "payload");
            let cwd = payload
                .and_then(|p| str_field(p, "cwd"))
                .or_else(|| str_field(&record, "cwd"))
                .map(str::to_string);
            let branch = payload
                .and_then(|p| obj_field(p, "git"))
                .and_then(|git| str_field(git, "branch"))
                .map(str::to_string);
            let root = cwd
                .as_deref()
                .and_then(|cwd| locate_pure_point_project_root(cwd, &mut cache));
            Conversation {
                project_path: cwd,
                pure_point_project_root: root,
                git_branch: branch,
                transcript_path: Some(file.to_string_lossy().into_owned()),
                ..session
            }
        })
        .collect()
}

/// The session id in a rollout file stem. Rollouts are named
/// `rollout-2025-01-01T12-00-00-<uuid>`, and a UUID has dashes of its own, so the id is the
/// trailing 36-character UUID. (The Mac takes the text after the last `-`, which is only
/// the UUID's final group and never matches a full id from the index.) Stems without a
/// trailing UUID keep the Mac rule.
fn session_id_from_stem(stem: &str) -> Option<String> {
    if let Some(tail) = stem.get(stem.len().saturating_sub(36)..)
        && tail.len() == 36
    {
        let is_uuid = tail.char_indices().all(|(i, c)| match i {
            8 | 13 | 18 | 23 => c == '-',
            _ => c.is_ascii_hexdigit(),
        });
        if is_uuid {
            return Some(tail.to_string());
        }
    }
    let after_dash = &stem[stem.rfind('-')? + 1..];
    (after_dash.len() >= 8).then(|| after_dash.to_string())
}

/// Session id → rollout path for every `.jsonl` under `<base>/sessions`.
fn session_file_paths(base: &Path) -> HashMap<String, PathBuf> {
    let mut map = HashMap::new();
    let mut stack = vec![base.join("sessions")];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.filter_map(Result::ok) {
            let name = entry.file_name().to_string_lossy().into_owned();
            if is_hidden(&name) {
                continue;
            }
            let path = entry.path();
            if entry.file_type().is_ok_and(|t| t.is_dir()) {
                stack.push(path);
            } else if let Some(stem) = name.strip_suffix(".jsonl")
                && let Some(id) = session_id_from_stem(stem)
            {
                map.insert(id, path);
            }
        }
    }
    map
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::conversations::test_support::TempDir;
    use serde_json::json;

    const ID_A: &str = "0199a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a5b";
    const ID_B: &str = "0199a1b2-c3d4-7e5f-8a9b-ffffffffffff";

    #[test]
    fn indexed_sessions_keep_first_entry_per_id() {
        let temp = TempDir::new();
        let lines = [
            json!({"id": ID_A, "thread_name": "Fix the build", "updated_at": "2026-11-25T20:00:00Z"}),
            json!({"id": ID_A, "thread_name": "Renamed later"}),
            json!({"id": ID_B}),
            json!({"thread_name": "no id"}),
        ];
        let text: Vec<String> = lines.iter().map(|l| l.to_string()).collect();
        temp.write(
            "session_index.jsonl",
            &format!("{}\n{{bad\n", text.join("\n")),
        );

        let sessions = load_indexed_sessions(temp.path()).unwrap();

        assert_eq!(sessions.len(), 2);
        assert_eq!(sessions[0].title, "Fix the build");
        assert_eq!(
            sessions[0].modified_at,
            parse_timestamp(Some("2026-11-25T20:00:00Z")).unwrap()
        );
        assert_eq!(sessions[1].title, "Codex session");
        assert_eq!(sessions[1].modified_at, DISTANT_PAST_MS);
        assert_eq!(sessions[1].id(), format!("codex:{ID_B}"));
    }

    #[test]
    fn missing_index_lists_nothing() {
        let temp = TempDir::new();
        assert!(load_indexed_sessions(temp.path()).unwrap().is_empty());
    }

    #[test]
    fn enrichment_reads_session_meta_from_rollouts() {
        let temp = TempDir::new();
        let project = temp.path().join("repo");
        let worktree = project.join(".pu").join("worktrees").join("wt-1");
        std::fs::create_dir_all(&worktree).unwrap();
        std::fs::write(project.join(".pu").join("manifest.json"), "{}").unwrap();
        let cwd = worktree.to_string_lossy().into_owned();
        let meta = json!({
            "timestamp": "2026-11-25T20:00:00.000Z",
            "type": "session_meta",
            "payload": {"id": ID_A, "cwd": cwd, "git": {"branch": "pu/wt-1"}},
        });
        temp.write(
            &format!("sessions/2026/11/25/rollout-2026-11-25T20-00-00-{ID_A}.jsonl"),
            &format!("{meta}\n{{\"type\":\"response_item\"}}\n"),
        );
        temp.write(
            "session_index.jsonl",
            &format!("{}\n{}\n", json!({"id": ID_A}), json!({"id": ID_B})),
        );

        let sessions = load_indexed_sessions(temp.path()).unwrap();
        let enriched = enrich_with_metadata(sessions.clone(), temp.path(), DEFAULT_ENRICH_LIMIT);

        assert_eq!(enriched[0].project_path.as_deref(), Some(cwd.as_str()));
        assert_eq!(enriched[0].git_branch.as_deref(), Some("pu/wt-1"));
        assert_eq!(
            enriched[0].pure_point_project_root,
            Some(project.to_string_lossy().into_owned())
        );
        assert!(
            enriched[0]
                .transcript_path
                .as_deref()
                .unwrap()
                .ends_with(".jsonl")
        );
        // No rollout: unchanged.
        assert_eq!(enriched[1], sessions[1]);
        // Past the limit: unchanged.
        assert_eq!(
            enrich_with_metadata(sessions.clone(), temp.path(), 0),
            sessions
        );
    }

    #[test]
    fn rollout_stems_yield_full_uuids() {
        assert_eq!(
            session_id_from_stem(&format!("rollout-2026-11-25T20-00-00-{ID_A}")).as_deref(),
            Some(ID_A)
        );
        assert_eq!(
            session_id_from_stem("rollout-legacy-abcdefgh").as_deref(),
            Some("abcdefgh")
        );
        assert_eq!(session_id_from_stem("rollout-short"), None);
    }
}
