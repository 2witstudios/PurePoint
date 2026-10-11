//! Native storage effects; normalization stays in pu-core.

use std::io::{BufRead, Read};
use std::path::{Path, PathBuf};

use pu_core::read::{Runtime, Transcript, parse_jsonl};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const MAX_SOURCE_BYTES: u64 = 64 * 1024 * 1024;
pub const MAX_DISCOVERY: usize = 10_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Binding {
    pub version: u8,
    pub runtime: Runtime,
    pub session_id: String,
    pub path: PathBuf,
    pub cwd: String,
}

#[derive(Debug, thiserror::Error)]
pub enum SourceError {
    #[error("native session identity is unavailable; use --session with its exact native ID")]
    IdentityUnavailable,
    #[error("multiple native sessions belong to this process; an exact binding is required")]
    Ambiguous,
    #[error("native session identity or working directory does not match")]
    IdentityMismatch,
    #[error("unsupported native storage schema: {0}")]
    UnsupportedSchema(String),
    #[error("native source unavailable: {0}")]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Parse(#[from] pu_core::read::ReadError),
    #[error("native database unavailable: {0}")]
    Database(#[from] rusqlite::Error),
}

pub fn valid_session_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// Pure launch argument policy. Identity is chosen at spawn, not at read time.
pub fn prepare_session_args(
    agent_type: &str,
    args: &mut Vec<String>,
    generated_id: &str,
) -> Result<Option<String>, SourceError> {
    let forbidden: &[&str] = match agent_type {
        "claude" => &["--session-id", "--resume", "--continue", "-r", "-c"],
        "opencode" => &["--session", "--continue", "-s", "-c"],
        "codex" => &["resume", "--last", "--session"],
        _ => return Ok(None),
    };
    if args.iter().any(|arg| {
        forbidden
            .iter()
            .any(|flag| arg == flag || arg.starts_with(&format!("{flag}=")))
    }) {
        return Err(SourceError::IdentityMismatch);
    }
    match agent_type {
        "claude" | "opencode" => {
            if !valid_session_id(generated_id) {
                return Err(SourceError::IdentityMismatch);
            }
            args.push(
                if agent_type == "claude" {
                    "--session-id"
                } else {
                    "--session"
                }
                .into(),
            );
            args.push(generated_id.into());
            Ok(Some(generated_id.into()))
        }
        "codex" => {
            if !args.iter().any(|arg| arg == "--no-daemon") {
                args.push("--no-daemon".into());
            }
            Ok(None)
        }
        _ => Ok(None),
    }
}

/// Pure storage-root policy using the environment supplied to the worker.
pub fn native_root(runtime: Runtime, env: &[(String, String)]) -> Result<PathBuf, SourceError> {
    let get = |key: &str| {
        env.iter()
            .rev()
            .find(|(k, _)| k == key)
            .map(|(_, v)| PathBuf::from(v))
    };
    let home = get("HOME").ok_or(SourceError::IdentityUnavailable)?;
    Ok(match runtime {
        Runtime::Claude => get("CLAUDE_CONFIG_DIR").unwrap_or_else(|| home.join(".claude")),
        Runtime::Codex => get("CODEX_HOME").unwrap_or_else(|| home.join(".codex")),
        Runtime::Opencode => get("XDG_DATA_HOME")
            .unwrap_or_else(|| home.join(".local/share"))
            .join("opencode"),
        Runtime::Pi => get("PI_CODING_AGENT_DIR").unwrap_or_else(|| home.join(".pi/agent")),
        Runtime::Terminal => return Err(SourceError::IdentityUnavailable),
    })
}

pub fn resolve_exact(
    runtime: Runtime,
    id: &str,
    cwd: &str,
    env: &[(String, String)],
) -> Result<Binding, SourceError> {
    if !valid_session_id(id) {
        return Err(SourceError::IdentityMismatch);
    }
    let root = native_root(runtime, env)?;
    let path = match runtime {
        Runtime::Claude => {
            let escaped: String = cwd
                .chars()
                .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
                .collect();
            root.join("projects")
                .join(escaped)
                .join(format!("{id}.jsonl"))
        }
        Runtime::Opencode => {
            let db = root.join("opencode.db");
            if db.exists() || !root.join("storage/session").exists() {
                db
            } else {
                let mut found = Vec::new();
                let mut visited = 0;
                for entry in std::fs::read_dir(root.join("storage/session"))? {
                    let entry = entry?;
                    visited += 1;
                    if visited > MAX_DISCOVERY {
                        return Err(pu_core::read::ReadError::TooLarge.into());
                    }
                    if entry.file_type()?.is_dir() {
                        let candidate = entry.path().join(format!("{id}.json"));
                        if candidate.is_file() {
                            found.push(candidate);
                        }
                    } else if entry.file_name() == format!("{id}.json").as_str() {
                        found.push(entry.path());
                    }
                }
                match found.len() {
                    0 => return Err(SourceError::IdentityUnavailable),
                    1 => found.remove(0),
                    _ => return Err(SourceError::Ambiguous),
                }
            }
        }
        Runtime::Codex | Runtime::Pi => {
            let mut pending = vec![root.join("sessions")];
            let mut visited = 0;
            let mut matches = Vec::new();
            while let Some(dir) = pending.pop() {
                for entry in std::fs::read_dir(dir)? {
                    let entry = entry?;
                    visited += 1;
                    if visited > MAX_DISCOVERY {
                        return Err(pu_core::read::ReadError::TooLarge.into());
                    }
                    let kind = entry.file_type()?;
                    if kind.is_dir() {
                        pending.push(entry.path());
                    } else if kind.is_file()
                        && entry.path().extension().is_some_and(|x| x == "jsonl")
                    {
                        if !entry.file_name().to_string_lossy().contains(id) {
                            continue;
                        }
                        let header = first_record(&entry.path())?;
                        let p = if runtime == Runtime::Codex {
                            &header["payload"]
                        } else {
                            &header
                        };
                        if p["id"] == id
                            && (runtime != Runtime::Codex || p["source"] == "cli")
                            && p["cwd"] == cwd
                        {
                            matches.push(entry.path());
                        }
                    }
                }
            }
            match matches.len() {
                0 => return Err(SourceError::IdentityUnavailable),
                1 => matches.remove(0),
                _ => return Err(SourceError::Ambiguous),
            }
        }
        _ => return Err(SourceError::IdentityUnavailable),
    };
    Ok(Binding {
        version: 1,
        runtime,
        session_id: id.into(),
        path,
        cwd: cwd.into(),
    })
}

fn binding_path(root: &Path, agent: &str) -> Result<PathBuf, SourceError> {
    if !valid_session_id(agent) {
        return Err(SourceError::IdentityMismatch);
    }
    Ok(root.join(".pu/read-bindings").join(format!("{agent}.json")))
}

pub fn load_binding(root: &Path, agent: &str) -> Result<Binding, SourceError> {
    let file = std::fs::File::open(binding_path(root, agent)?)?;
    let mut bytes = Vec::new();
    file.take(8193).read_to_end(&mut bytes)?;
    if bytes.len() > 8192 {
        return Err(pu_core::read::ReadError::TooLarge.into());
    }
    serde_json::from_slice(&bytes).map_err(|e| SourceError::UnsupportedSchema(e.to_string()))
}

pub fn save_binding(root: &Path, agent: &str, binding: &Binding) -> Result<(), SourceError> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let path = binding_path(root, agent)?;
    let dir = path.parent().unwrap();
    std::fs::create_dir_all(dir)?;
    let temporary = dir.join(format!(".{agent}-{}.tmp", pu_core::id::session_id()));
    let result = (|| -> Result<(), SourceError> {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temporary)?;
        file.write_all(
            &serde_json::to_vec(binding)
                .map_err(|e| SourceError::UnsupportedSchema(e.to_string()))?,
        )?;
        file.sync_all()?;
        std::fs::rename(&temporary, &path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(temporary);
    }
    result
}

/// The manifest lock serializes ownership checks with stop/resume publication.
pub fn commit_binding(
    root: &Path,
    agent: &str,
    expected_pid: Option<u32>,
    expected_session: Option<&str>,
    binding: &Binding,
) -> Result<(), SourceError> {
    let mut result = Err(SourceError::IdentityMismatch);
    pu_core::manifest::update_manifest(root, |mut m| {
        if let Some(a) = m.find_agent_mut(agent) {
            if a.pid == expected_pid && a.session_id.as_deref() == expected_session {
                result = save_binding(root, agent, binding);
                if result.is_ok() {
                    a.session_id = Some(binding.session_id.clone());
                }
            }
        }
        m
    })
    .map_err(|e| SourceError::Io(std::io::Error::other(e)))?;
    result
}

/// Only descriptors held by the owned process tree are discovery candidates.
pub async fn discover_codex(pid: u32, cwd: &str) -> Result<Binding, SourceError> {
    use tokio::io::AsyncReadExt;
    let tree = crate::pty_manager::snapshot_process_tree().await;
    let mut pids = crate::pty_manager::descendants_from_tree(&tree, pid as i32);
    pids.push(pid as i32);
    if pids.len() > 128 {
        return Err(pu_core::read::ReadError::TooLarge.into());
    }
    let pids = pids
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(",");
    let mut child = tokio::process::Command::new("lsof")
        .args(["-a", "-p", &pids, "-Fn"])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()?;
    let mut stdout = child.stdout.take().unwrap().take(2 * 1024 * 1024 + 1);
    let mut bytes = Vec::new();
    tokio::time::timeout(
        std::time::Duration::from_secs(2),
        stdout.read_to_end(&mut bytes),
    )
    .await
    .map_err(|_| SourceError::IdentityUnavailable)??;
    if bytes.len() > 2 * 1024 * 1024 {
        return Err(pu_core::read::ReadError::TooLarge.into());
    }
    let _ = tokio::time::timeout(std::time::Duration::from_secs(1), child.wait()).await;
    let paths: Vec<_> = String::from_utf8_lossy(&bytes)
        .lines()
        .filter_map(|line| line.strip_prefix('n'))
        .map(PathBuf::from)
        .filter(|p| {
            p.file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with("rollout-"))
                && p.extension().is_some_and(|x| x == "jsonl")
        })
        .collect();
    select_codex_source(&paths, cwd)
}

fn first_record(path: &Path) -> Result<Value, SourceError> {
    let file = std::fs::File::open(path)?;
    let mut line = String::new();
    std::io::BufReader::new(file.take(1024 * 1024 + 1)).read_line(&mut line)?;
    if line.len() > 1024 * 1024 {
        return Err(pu_core::read::ReadError::TooLarge.into());
    }
    serde_json::from_str(&line).map_err(|e| SourceError::UnsupportedSchema(e.to_string()))
}

/// Candidate paths must come from the owned process, not a directory timestamp.
pub fn select_codex_source(paths: &[PathBuf], cwd: &str) -> Result<Binding, SourceError> {
    if paths.len() > MAX_DISCOVERY {
        return Err(pu_core::read::ReadError::TooLarge.into());
    }
    let mut bindings = std::collections::BTreeMap::new();
    for path in paths {
        let header = first_record(path)?;
        let p = &header["payload"];
        if header["type"] != "session_meta" || p["source"] != "cli" {
            continue;
        }
        if p["cwd"].as_str() != Some(cwd) {
            continue;
        }
        let Some(id) = p["id"].as_str().filter(|id| valid_session_id(id)) else {
            return Err(SourceError::IdentityMismatch);
        };
        if p.get("session_id").is_some_and(|v| v.as_str() != Some(id)) {
            return Err(SourceError::IdentityMismatch);
        }
        bindings.insert(
            path.clone(),
            Binding {
                version: 1,
                runtime: Runtime::Codex,
                session_id: id.to_owned(),
                path: path.clone(),
                cwd: cwd.to_owned(),
            },
        );
    }
    match bindings.len() {
        0 => Err(SourceError::IdentityUnavailable),
        1 => Ok(bindings.into_values().next().unwrap()),
        _ => Err(SourceError::Ambiguous),
    }
}

pub fn read_jsonl(binding: &Binding) -> Result<Transcript, SourceError> {
    if binding.version != 1 || !valid_session_id(&binding.session_id) {
        return Err(SourceError::IdentityMismatch);
    }
    let file = std::fs::File::open(&binding.path)?;
    let mut bytes = Vec::new();
    file.take(MAX_SOURCE_BYTES + 1).read_to_end(&mut bytes)?;
    let first_line = bytes.split(|b| *b == b'\n').next().unwrap_or_default();
    if first_line.len() > 1024 * 1024 {
        return Err(pu_core::read::ReadError::TooLarge.into());
    }
    let header: Value = serde_json::from_slice(first_line)
        .map_err(|e| SourceError::UnsupportedSchema(e.to_string()))?;
    let matches = match binding.runtime {
        Runtime::Codex => {
            header["type"] == "session_meta"
                && header["payload"]["id"] == binding.session_id
                && header["payload"]["cwd"] == binding.cwd
                && header["payload"]["source"] == "cli"
        }
        Runtime::Pi => {
            header["type"] == "session"
                && header["id"] == binding.session_id
                && header["cwd"] == binding.cwd
        }
        Runtime::Claude => {
            binding.path.file_stem().and_then(|s| s.to_str()) == Some(&binding.session_id)
                && header
                    .get("sessionId")
                    .is_none_or(|id| id.as_str() == Some(&binding.session_id))
        }
        _ => false,
    };
    if !matches {
        return Err(SourceError::IdentityMismatch);
    }
    Ok(parse_jsonl(binding.runtime, &bytes)?)
}

pub fn read_opencode(binding: &Binding) -> Result<Transcript, SourceError> {
    if binding.version != 1 || !valid_session_id(&binding.session_id) {
        return Err(SourceError::IdentityMismatch);
    }
    if binding.path.extension().is_some_and(|x| x == "json") {
        return read_opencode_files(binding);
    }
    let mut db = rusqlite::Connection::open_with_flags(
        &binding.path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )?;
    db.busy_timeout(std::time::Duration::from_secs(1))?;
    let tx = db.transaction()?;
    let v2: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='session_v2')",
        [],
        |row| row.get(0),
    )?;
    let session_query = if v2 {
        "SELECT directory FROM session_v2 WHERE id=?1"
    } else {
        "SELECT directory FROM session WHERE id=?1"
    };
    let directory: String = tx.query_row(session_query, [&binding.session_id], |row| row.get(0))?;
    if directory != binding.cwd {
        return Err(SourceError::IdentityMismatch);
    }
    let query = if v2 {
        "SELECT id,data FROM session_message WHERE session_id=?1 ORDER BY seq,id LIMIT 100001"
    } else {
        "SELECT id,data FROM message WHERE session_id=?1 ORDER BY time_created,id LIMIT 100001"
    };
    let mut statement = tx.prepare(query)?;
    let mut rows = statement.query([&binding.session_id])?;
    let mut messages = Vec::new();
    let mut total_bytes = 0;
    while let Some(row) = rows.next()? {
        if messages.len() >= 100_000 {
            return Err(pu_core::read::ReadError::TooLarge.into());
        }
        let id: String = row.get(0)?;
        let data: String = row.get(1)?;
        total_bytes += data.len() as u64;
        if total_bytes > MAX_SOURCE_BYTES || data.len() > 1024 * 1024 {
            return Err(pu_core::read::ReadError::TooLarge.into());
        }
        let mut message: Value = serde_json::from_str(&data)
            .map_err(|e| SourceError::UnsupportedSchema(e.to_string()))?;
        if !message.is_object() {
            return Err(SourceError::UnsupportedSchema(
                "message must be an object".into(),
            ));
        }
        message["id"] = Value::String(id.clone());
        if v2 {
            messages.push(message);
        } else {
            let mut parts_statement = tx.prepare("SELECT id,data FROM part WHERE message_id=?1 ORDER BY time_created,id LIMIT 100001")?;
            let mut parts_rows = parts_statement.query([&id])?;
            let mut parts = Vec::new();
            while let Some(row) = parts_rows.next()? {
                let id: String = row.get(0)?;
                let data: String = row.get(1)?;
                total_bytes += data.len() as u64;
                if total_bytes > MAX_SOURCE_BYTES
                    || data.len() > 1024 * 1024
                    || parts.len() >= 100_000
                {
                    return Err(pu_core::read::ReadError::TooLarge.into());
                }
                let mut part: Value = serde_json::from_str(&data)
                    .map_err(|e| SourceError::UnsupportedSchema(e.to_string()))?;
                if !part.is_object() {
                    return Err(SourceError::UnsupportedSchema(
                        "part must be an object".into(),
                    ));
                }
                part["id"] = Value::String(id);
                parts.push(part);
            }
            messages.push(serde_json::json!({"info":message,"parts":parts}));
        }
    }
    Ok(pu_core::read::parse_opencode(
        &serde_json::json!({"messages":messages}),
    )?)
}

fn read_json_bounded(path: &Path, total: &mut u64) -> Result<Value, SourceError> {
    let file = std::fs::File::open(path)?;
    let mut bytes = Vec::new();
    file.take(1024 * 1024 + 1).read_to_end(&mut bytes)?;
    *total += bytes.len() as u64;
    if bytes.len() > 1024 * 1024 || *total > MAX_SOURCE_BYTES {
        return Err(pu_core::read::ReadError::TooLarge.into());
    }
    serde_json::from_slice(&bytes).map_err(|e| SourceError::UnsupportedSchema(e.to_string()))
}

fn json_files(dir: &Path) -> Result<Vec<PathBuf>, SourceError> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(e) => return Err(e.into()),
    };
    let mut paths = Vec::new();
    let mut count = 0;
    for entry in entries {
        let entry = entry?;
        count += 1;
        if count > MAX_DISCOVERY {
            return Err(pu_core::read::ReadError::TooLarge.into());
        }
        if entry.file_type()?.is_file() && entry.path().extension().is_some_and(|x| x == "json") {
            paths.push(entry.path());
        }
    }
    paths.sort();
    Ok(paths)
}

fn read_opencode_files(binding: &Binding) -> Result<Transcript, SourceError> {
    let mut total = 0;
    let session = read_json_bounded(&binding.path, &mut total)?;
    if session["id"] != binding.session_id || session["directory"] != binding.cwd {
        return Err(SourceError::IdentityMismatch);
    }
    let storage = binding
        .path
        .ancestors()
        .find(|p| p.file_name().is_some_and(|x| x == "storage"))
        .ok_or(SourceError::IdentityMismatch)?;
    let mut infos = Vec::new();
    for path in json_files(&storage.join("message").join(&binding.session_id))? {
        let info = read_json_bounded(&path, &mut total)?;
        if info["sessionID"] != binding.session_id
            || info["id"].as_str().is_none_or(|id| !valid_session_id(id))
        {
            return Err(SourceError::IdentityMismatch);
        }
        infos.push(info);
    }
    infos.sort_by(|a, b| {
        a["time"]["created"]
            .as_i64()
            .cmp(&b["time"]["created"].as_i64())
            .then_with(|| a["id"].as_str().cmp(&b["id"].as_str()))
    });
    let mut messages = Vec::new();
    for info in infos {
        let id = info["id"].as_str().unwrap();
        let mut parts = Vec::new();
        for path in json_files(&storage.join("part").join(id))? {
            let part = read_json_bounded(&path, &mut total)?;
            if part["sessionID"] != binding.session_id || part["messageID"] != id {
                return Err(SourceError::IdentityMismatch);
            }
            parts.push(part);
        }
        messages.push(serde_json::json!({"info":info,"parts":parts}));
    }
    Ok(pu_core::read::parse_opencode(
        &serde_json::json!({"messages":messages}),
    )?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_legacy_opencode_json_should_read_the_exact_session_in_native_order() {
        // given
        let temp = tempfile::tempdir().unwrap();
        let storage = temp.path().join("storage");
        for dir in [
            "session/project",
            "message/ses_exact",
            "part/msg_a",
            "part/msg_b",
        ] {
            std::fs::create_dir_all(storage.join(dir)).unwrap();
        }
        std::fs::write(
            storage.join("session/project/ses_exact.json"),
            serde_json::json!({"id":"ses_exact","directory":"/project"}).to_string(),
        )
        .unwrap();
        for (id, timestamp, text) in [("msg_b", 2, "Answer"), ("msg_a", 1, "Question")] {
            std::fs::write(storage.join(format!("message/ses_exact/{id}.json")), serde_json::json!({"id":id,"sessionID":"ses_exact","role":if timestamp==1 {"user"} else {"assistant"},"time":{"created":timestamp}}).to_string()).unwrap();
            std::fs::write(storage.join(format!("part/{id}/part.json")), serde_json::json!({"id":"part","sessionID":"ses_exact","messageID":id,"type":"text","text":text}).to_string()).unwrap();
        }
        let binding = Binding {
            version: 1,
            runtime: Runtime::Opencode,
            session_id: "ses_exact".into(),
            path: storage.join("session/project/ses_exact.json"),
            cwd: "/project".into(),
        };
        // when
        let result = read_opencode(&binding).unwrap();
        // then
        assert_eq!(
            result
                .records
                .iter()
                .map(|r| r.text.as_str())
                .collect::<Vec<_>>(),
            ["Question", "Answer"]
        );
    }

    #[test]
    fn given_replaced_worker_should_reject_a_stale_binding_commit() {
        // given
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join(".pu")).unwrap();
        pu_core::manifest::write_manifest(
            temp.path(),
            &pu_core::types::Manifest::new(temp.path().to_string_lossy().into_owned()),
        )
        .unwrap();
        pu_core::manifest::update_manifest(temp.path(), |mut m| {
            let a = serde_json::from_value(serde_json::json!({"id":"ag-one","name":"one","agentType":"codex","status":"running","startedAt":"2026-10-10T00:00:00Z","pid":22})).unwrap();
            m.agents.insert("ag-one".into(), a); m
        }).unwrap();
        let binding = Binding {
            version: 1,
            runtime: Runtime::Codex,
            session_id: "old".into(),
            path: temp.path().join("old.jsonl"),
            cwd: temp.path().to_string_lossy().into_owned(),
        };
        // when
        let result = commit_binding(temp.path(), "ag-one", Some(11), None, &binding);
        // then
        assert!(matches!(result, Err(SourceError::IdentityMismatch)));
        assert!(load_binding(temp.path(), "ag-one").is_err());
        commit_binding(temp.path(), "ag-one", Some(22), None, &binding).unwrap();
        assert_eq!(
            load_binding(temp.path(), "ag-one").unwrap().session_id,
            "old"
        );
    }

    #[test]
    fn given_native_roots_should_resolve_exact_ids_and_preserve_bindings() {
        // given
        let temp = tempfile::tempdir().unwrap();
        let env = vec![
            ("HOME".into(), temp.path().to_string_lossy().into_owned()),
            (
                "CLAUDE_CONFIG_DIR".into(),
                temp.path().join("custom").to_string_lossy().into_owned(),
            ),
        ];
        let cwd = "/workspace/project";
        // when
        let binding = resolve_exact(Runtime::Claude, "exact", cwd, &env).unwrap();
        save_binding(temp.path(), "ag-one", &binding).unwrap();
        let loaded = load_binding(temp.path(), "ag-one").unwrap();
        // then
        assert_eq!(
            binding.path,
            temp.path()
                .join("custom/projects/-workspace-project/exact.jsonl")
        );
        assert_eq!(loaded.session_id, "exact");
        assert!(resolve_exact(Runtime::Claude, "../other", cwd, &env).is_err());
    }

    #[test]
    fn given_opencode_sqlite_should_read_only_the_exact_session_and_detect_live_updates() {
        // given
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("opencode.db");
        let db = rusqlite::Connection::open(&path).unwrap();
        db.execute_batch("CREATE TABLE session_v2(id TEXT PRIMARY KEY,directory TEXT); CREATE TABLE session_message(id TEXT PRIMARY KEY,session_id TEXT,type TEXT,seq INTEGER,data TEXT);").unwrap();
        let cwd = temp.path().to_str().unwrap();
        for id in ["ses_exact", "ses_other"] {
            db.execute(
                "INSERT INTO session_v2 VALUES(?1,?2)",
                rusqlite::params![id, cwd],
            )
            .unwrap();
        }
        db.execute(
            "INSERT INTO session_message VALUES('m1','ses_exact','user',1,?1)",
            [r#"{"id":"m1","type":"user","text":"Question"}"#],
        )
        .unwrap();
        db.execute("INSERT INTO session_message VALUES('m2','ses_other','assistant',2,?1)", [r#"{"id":"m2","type":"assistant","content":[{"type":"text","text":"Other session"}]}"#]).unwrap();
        let binding = Binding {
            version: 1,
            runtime: Runtime::Opencode,
            session_id: "ses_exact".into(),
            path,
            cwd: cwd.into(),
        };
        // when
        let first = read_opencode(&binding).unwrap();
        let cursor = pu_core::read::paginate("exact", &first, None, 10)
            .unwrap()
            .next_cursor;
        db.execute(
            "UPDATE session_message SET data=?1 WHERE id='m1'",
            [r#"{"id":"m1","type":"user","text":"Updated"}"#],
        )
        .unwrap();
        let changed = read_opencode(&binding).unwrap();
        // then
        assert_eq!(first.records.len(), 1);
        assert_eq!(first.records[0].text, "Question");
        assert!(matches!(
            pu_core::read::paginate("exact", &changed, Some(&cursor), 10),
            Err(pu_core::read::ReadError::StaleCursor)
        ));
    }

    #[test]
    fn given_native_launch_should_bind_explicit_ids_and_reject_conflicting_options() {
        // given
        let mut opencode = vec!["--model".into(), "model".into()];
        let mut codex = vec!["--sandbox".into(), "workspace-write".into()];
        // when
        let id = prepare_session_args("opencode", &mut opencode, "ses_exact").unwrap();
        prepare_session_args("codex", &mut codex, "unused").unwrap();
        // then
        assert_eq!(id.as_deref(), Some("ses_exact"));
        assert_eq!(&opencode[2..], &["--session", "ses_exact"]);
        assert_eq!(codex.last().map(String::as_str), Some("--no-daemon"));
        let mut conflict = vec!["--session=ses_other".into()];
        assert!(prepare_session_args("opencode", &mut conflict, "ses_exact").is_err());
    }

    #[test]
    fn given_exact_codex_source_should_verify_identity_and_ignore_delegated_threads() {
        // given
        let temp = tempfile::tempdir().unwrap();
        let cwd = temp.path().to_string_lossy().into_owned();
        let root = temp.path().join("rollout-root.jsonl");
        let child = temp.path().join("rollout-child.jsonl");
        std::fs::write(&root, format!("{{\"type\":\"session_meta\",\"payload\":{{\"id\":\"root\",\"cwd\":{cwd:?},\"source\":\"cli\"}}}}\n")).unwrap();
        std::fs::write(&child, format!("{{\"type\":\"session_meta\",\"payload\":{{\"id\":\"child\",\"session_id\":\"root\",\"cwd\":{cwd:?},\"source\":{{\"subagent\":{{}}}}}}}}\n")).unwrap();
        // when
        let binding = select_codex_source(&[child, root.clone()], &cwd).unwrap();
        // then
        assert_eq!(binding.session_id, "root");
        assert_eq!(binding.path, root);
        let other = temp.path().join("rollout-other.jsonl");
        std::fs::write(&other, format!("{{\"type\":\"session_meta\",\"payload\":{{\"id\":\"other\",\"cwd\":{cwd:?},\"source\":\"cli\"}}}}\n")).unwrap();
        assert!(matches!(
            select_codex_source(&[binding.path, other], &cwd),
            Err(SourceError::Ambiguous)
        ));
    }
}
