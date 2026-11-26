//! Local text files with modification times, for the layout document (WIN-6.0).
//!
//! `.pu/workspaces.json` is written by this app, the Mac app working on the same project over
//! SSH, and `pu grid`. The front end's cached persistence adapter reads it with its modification
//! time and writes it only if the file still has the time it last saw, so another writer's change
//! comes back as a conflict to merge instead of being overwritten (Swift's `saveLocal` over
//! `LocalProjectHost`). Remote projects never come here: they go through their daemon's `fs_*`
//! requests.

use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use serde::Serialize;

/// The most bytes read when the caller gives no limit (the layout cap, 4 MiB).
pub const DEFAULT_MAX_BYTES: u64 = 4 * 1_048_576;

/// A text file as read: lossy UTF-8, at most `max_bytes` of it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TextFile {
    pub text: String,
    pub modified_ms: u64,
    /// The file was longer than `max_bytes`, so `text` is incomplete.
    pub truncated: bool,
}

/// How a file command fails. `conflict`: the file's modification time is not the expected one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", content = "message", rename_all = "snake_case")]
pub enum FsError {
    Conflict(String),
    Io(String),
}

impl FsError {
    fn io(path: &Path, error: &io::Error) -> Self {
        FsError::Io(format!("{}: {error}", path.display()))
    }
}

fn modified_ms(metadata: &std::fs::Metadata) -> io::Result<u64> {
    let modified = metadata.modified()?;
    let since = modified.duration_since(UNIX_EPOCH).unwrap_or_default();
    Ok(u64::try_from(since.as_millis()).unwrap_or(u64::MAX))
}

/// The file's modification time, `None` when there is no file.
pub fn stat(path: &Path) -> Result<Option<u64>, FsError> {
    match std::fs::metadata(path) {
        Ok(metadata) => modified_ms(&metadata)
            .map(Some)
            .map_err(|e| FsError::io(path, &e)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(FsError::io(path, &e)),
    }
}

/// The file's text, `None` when there is no file.
pub fn read(path: &Path, max_bytes: u64) -> Result<Option<TextFile>, FsError> {
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(FsError::io(path, &e)),
    };
    let metadata = file.metadata().map_err(|e| FsError::io(path, &e))?;
    let modified_ms = modified_ms(&metadata).map_err(|e| FsError::io(path, &e))?;
    let mut bytes = Vec::new();
    file.take(max_bytes.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|e| FsError::io(path, &e))?;
    let truncated = bytes.len() as u64 > max_bytes;
    if truncated {
        bytes.truncate(usize::try_from(max_bytes).unwrap_or(usize::MAX));
    }
    Ok(Some(TextFile {
        text: String::from_utf8_lossy(&bytes).into_owned(),
        modified_ms,
        truncated,
    }))
}

fn temp_path(path: &Path) -> PathBuf {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    path.with_file_name(format!(".{name}.{}.tmp", uuid::Uuid::new_v4().simple()))
}

/// Write `text` unless the file changed: its modification time must still be
/// `expected_modified_ms` (`None`: there must be no file). Creates the parent directory and
/// replaces the file through a temporary sibling, so readers never see half a document.
/// Returns the new modification time.
pub fn write_if_unmodified(
    path: &Path,
    text: &str,
    expected_modified_ms: Option<u64>,
) -> Result<u64, FsError> {
    let current = stat(path)?;
    if current != expected_modified_ms {
        return Err(FsError::Conflict(format!(
            "{} changed since it was read.",
            path.display()
        )));
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| FsError::io(parent, &e))?;
    }
    let temp = temp_path(path);
    if let Err(e) = std::fs::write(&temp, text.as_bytes()) {
        let _ = std::fs::remove_file(&temp);
        return Err(FsError::io(&temp, &e));
    }
    // `rename` replaces an existing file on Windows (MOVEFILE_REPLACE_EXISTING).
    if let Err(e) = std::fs::rename(&temp, path) {
        let _ = std::fs::remove_file(&temp);
        return Err(FsError::io(path, &e));
    }
    stat(path)?.ok_or_else(|| FsError::Io(format!("{} vanished after writing.", path.display())))
}

async fn blocking<T: Send + 'static>(
    body: impl FnOnce() -> Result<T, FsError> + Send + 'static,
) -> Result<T, FsError> {
    tauri::async_runtime::spawn_blocking(body)
        .await
        .map_err(|e| FsError::Io(e.to_string()))?
}

/// `fs_stat_text(path)`: the modification time, or null when there is no file.
#[tauri::command]
pub async fn fs_stat_text(path: String) -> Result<Option<u64>, FsError> {
    blocking(move || stat(Path::new(&path))).await
}

/// `fs_read_text(path, maxBytes)`: the file's text, or null when there is no file.
#[tauri::command]
pub async fn fs_read_text(
    path: String,
    max_bytes: Option<u64>,
) -> Result<Option<TextFile>, FsError> {
    let max = max_bytes.unwrap_or(DEFAULT_MAX_BYTES);
    blocking(move || read(Path::new(&path), max)).await
}

/// `fs_write_text_if_unmodified(path, text, expectedModifiedMs)`: the new modification time.
#[tauri::command]
pub async fn fs_write_text_if_unmodified(
    path: String,
    text: String,
    expected_modified_ms: Option<u64>,
) -> Result<u64, FsError> {
    blocking(move || write_if_unmodified(Path::new(&path), &text, expected_modified_ms)).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layout(dir: &tempfile::TempDir) -> PathBuf {
        dir.path().join(".pu").join("workspaces.json")
    }

    #[test]
    fn given_missing_file_should_stat_and_read_none() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(stat(&layout(&dir)), Ok(None));
        assert_eq!(read(&layout(&dir), 10), Ok(None));
    }

    #[test]
    fn given_no_file_and_no_expectation_should_create_parent_and_write() {
        let dir = tempfile::tempdir().unwrap();
        let path = layout(&dir);
        let modified = write_if_unmodified(&path, "{\"version\":4}", None).unwrap();
        let file = read(&path, DEFAULT_MAX_BYTES).unwrap().unwrap();
        assert_eq!(file.text, "{\"version\":4}");
        assert_eq!(file.modified_ms, modified);
        assert!(!file.truncated);
        assert_eq!(stat(&path), Ok(Some(modified)));
    }

    #[test]
    fn given_expected_time_should_replace_the_file_without_leaving_temporaries() {
        let dir = tempfile::tempdir().unwrap();
        let path = layout(&dir);
        let first = write_if_unmodified(&path, "one", None).unwrap();
        write_if_unmodified(&path, "two", Some(first)).unwrap();
        assert_eq!(read(&path, 100).unwrap().unwrap().text, "two");
        let names: Vec<_> = std::fs::read_dir(path.parent().unwrap())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names, vec![std::ffi::OsString::from("workspaces.json")]);
    }

    #[test]
    fn given_changed_file_should_return_conflict_and_keep_it() {
        let dir = tempfile::tempdir().unwrap();
        let path = layout(&dir);
        let first = write_if_unmodified(&path, "theirs", None).unwrap();
        let stale = first.wrapping_sub(5_000);
        assert!(matches!(
            write_if_unmodified(&path, "ours", Some(stale)),
            Err(FsError::Conflict(_))
        ));
        assert!(matches!(
            write_if_unmodified(&path, "ours", None),
            Err(FsError::Conflict(_))
        ));
        assert_eq!(read(&path, 100).unwrap().unwrap().text, "theirs");
    }

    #[test]
    fn given_deleted_file_with_expectation_should_return_conflict() {
        let dir = tempfile::tempdir().unwrap();
        let path = layout(&dir);
        let first = write_if_unmodified(&path, "one", None).unwrap();
        std::fs::remove_file(&path).unwrap();
        assert!(matches!(
            write_if_unmodified(&path, "two", Some(first)),
            Err(FsError::Conflict(_))
        ));
    }

    #[test]
    fn given_long_file_should_truncate_to_max_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let path = layout(&dir);
        write_if_unmodified(&path, "abcdef", None).unwrap();
        let file = read(&path, 4).unwrap().unwrap();
        assert_eq!(file.text, "abcd");
        assert!(file.truncated);
        assert!(!read(&path, 6).unwrap().unwrap().truncated);
    }

    #[test]
    fn given_errors_should_serialize_kind_and_message() {
        let json = serde_json::to_value(FsError::Conflict("x".into())).unwrap();
        assert_eq!(
            json,
            serde_json::json!({ "kind": "conflict", "message": "x" })
        );
        let json = serde_json::to_value(TextFile {
            text: "t".into(),
            modified_ms: 3,
            truncated: false,
        })
        .unwrap();
        assert_eq!(
            json,
            serde_json::json!({ "text": "t", "modifiedMs": 3, "truncated": false })
        );
    }
}
