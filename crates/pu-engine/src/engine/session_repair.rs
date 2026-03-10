use std::collections::HashSet;
use std::io::BufRead;
use std::os::fd::OwnedFd;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::pty_manager::NativePtyHost;

use super::Engine;

impl Engine {
    /// Scan Claude Code's session directory for the latest continuation of a session.
    /// Claude Code stores sessions at `~/.claude/projects/{escaped-cwd}/{uuid}.jsonl`.
    /// Resolve the sessions directory for a given working directory.
    pub(super) fn sessions_dir_for(cwd: &str) -> Option<PathBuf> {
        let home = std::env::var("HOME").ok()?;
        let escaped: String = cwd
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '-' {
                    c
                } else {
                    '-'
                }
            })
            .collect();
        Some(
            PathBuf::from(&home)
                .join(".claude")
                .join("projects")
                .join(&escaped),
        )
    }

    /// Repair corrupted Claude Code session JSONL files for the given session.
    ///
    /// Fixes three known corruption patterns (claude-code#24304):
    /// 1. Snapshot `messageId` collisions — `file-history-snapshot` entries sharing UUIDs with real messages
    /// 2. Broken `parentUuid` references — entries pointing to non-existent UUIDs
    /// 3. Disconnected compaction roots — multiple `parentUuid: null` entries splitting the conversation
    pub(super) fn repair_session_files(cwd: &str, session_id: &str) {
        let Some(sessions_dir) = Self::sessions_dir_for(cwd) else {
            return;
        };
        if !sessions_dir.is_dir() {
            return;
        }

        // Repair the original session file
        let original = sessions_dir.join(format!("{session_id}.jsonl"));
        if original.is_file() {
            repair_session_file(&original);
        }

        // Repair continuation files that chain back to the original session
        let Ok(entries) = std::fs::read_dir(&sessions_dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                continue;
            }
            // Skip the original — already repaired
            if path.file_stem().and_then(|s| s.to_str()) == Some(session_id) {
                continue;
            }
            // Only repair continuation files (first line has non-null parentUuid)
            let Ok(file) = std::fs::File::open(&path) else {
                continue;
            };
            let mut reader = std::io::BufReader::new(file);
            let mut first_line = String::new();
            if BufRead::read_line(&mut reader, &mut first_line).is_err() {
                continue;
            }
            let Ok(value) = serde_json::from_str::<serde_json::Value>(&first_line) else {
                continue;
            };
            if value.get("parentUuid").and_then(|v| v.as_str()).is_some() {
                repair_session_file(&path);
            }
        }
    }
}

/// Inject prompt text into a PTY and submit with Enter via chunked typing.
/// Returns `true` on success.
pub(super) async fn inject_initial_prompt(
    pty_host: &NativePtyHost,
    master_fd: &Arc<OwnedFd>,
    agent_id: &str,
    prompt: &[u8],
) -> bool {
    if prompt.is_empty() {
        return true;
    }
    if let Err(e) = pty_host.write_chunked_submit(master_fd, prompt).await {
        tracing::warn!("failed to inject initial prompt for {}: {}", agent_id, e);
        return false;
    }
    true
}

/// Repair a single Claude Code session JSONL file.
///
/// Returns `true` if any repairs were made (a `.bak` backup is written).
pub(super) fn repair_session_file(path: &Path) -> bool {
    let Ok(content) = std::fs::read_to_string(path) else {
        return false;
    };

    let mut lines: Vec<serde_json::Value> = Vec::new();
    for raw_line in content.lines() {
        if raw_line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str::<serde_json::Value>(raw_line) {
            Ok(v) => lines.push(v),
            Err(_) => {
                // Preserve unparseable lines as-is by wrapping in a raw marker
                lines.push(serde_json::json!({"__raw": raw_line}));
            }
        }
    }

    if lines.is_empty() {
        return false;
    }

    // Collect all "uuid" values into a set
    let mut uuid_set: HashSet<String> = HashSet::new();
    for entry in &lines {
        if let Some(uuid) = entry.get("uuid").and_then(|v| v.as_str()) {
            uuid_set.insert(uuid.to_string());
        }
    }

    let mut modified = false;

    // Fix 1: Snapshot messageId collisions
    // file-history-snapshot entries sometimes reuse a messageId that collides with
    // a real message uuid. Nullify the messageId to prevent confusion.
    for entry in &mut lines {
        if entry.get("__raw").is_some() {
            continue;
        }
        let is_snapshot =
            entry.get("type").and_then(|v| v.as_str()) == Some("file-history-snapshot");
        if !is_snapshot {
            continue;
        }
        if let Some(mid) = entry
            .get("messageId")
            .and_then(|v| v.as_str())
            .map(String::from)
        {
            if uuid_set.contains(&mid) {
                entry["messageId"] = serde_json::Value::Null;
                modified = true;
            }
        }
    }

    // Fix 2: Broken parentUuid references — point to nearest preceding entry's uuid
    // Fix 3: Disconnected roots — if >1 entry has parentUuid: null, stitch extras
    let mut null_parent_count = 0;
    let mut last_uuid: Option<String> = None;

    for entry in &mut lines {
        if entry.get("__raw").is_some() {
            continue;
        }

        let has_parent_uuid_field = entry.get("parentUuid").is_some();
        let parent_uuid_value = entry
            .get("parentUuid")
            .and_then(|v| v.as_str())
            .map(String::from);

        if has_parent_uuid_field {
            match &parent_uuid_value {
                Some(pu) if !uuid_set.contains(pu) => {
                    // Broken reference — point to nearest preceding uuid
                    if let Some(ref prev) = last_uuid {
                        entry["parentUuid"] = serde_json::Value::String(prev.clone());
                        modified = true;
                    }
                }
                None => {
                    // parentUuid is null — this is a root
                    null_parent_count += 1;
                    if null_parent_count > 1 {
                        // Stitch disconnected root to nearest preceding uuid
                        if let Some(ref prev) = last_uuid {
                            entry["parentUuid"] = serde_json::Value::String(prev.clone());
                            modified = true;
                        }
                    }
                }
                _ => {}
            }
        }

        // Track the most recent uuid for stitching
        if let Some(uuid) = entry.get("uuid").and_then(|v| v.as_str()) {
            last_uuid = Some(uuid.to_string());
        }
    }

    if !modified {
        return false;
    }

    // Write backup
    let backup = path.with_extension("jsonl.bak");
    let _ = std::fs::write(&backup, &content);

    // Write repaired file
    let mut output = String::new();
    for entry in &lines {
        if let Some(raw) = entry.get("__raw").and_then(|v| v.as_str()) {
            output.push_str(raw);
        } else {
            output.push_str(&serde_json::to_string(entry).unwrap_or_default());
        }
        output.push('\n');
    }
    let _ = std::fs::write(path, &output);

    true
}
