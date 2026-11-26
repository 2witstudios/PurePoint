//! Local text files with modification times, for the layout document (WIN-6.0) and the editor
//! (WIN-8.4: `read_document`).
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

use super::roots::Access;

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

/// A file opened in the editor or a preview (WIN-8.4): strictly decoded, so a binary file is
/// reported as one instead of shown as replacement characters.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentFile {
    /// Empty when `is_binary`.
    pub text: String,
    /// `FileIOService.decode`: a NUL byte in the first 8 KB, or bytes that are not UTF-8.
    pub is_binary: bool,
    pub modified_ms: u64,
    /// The file was longer than `max_bytes`; the caller treats it as too large to open.
    pub truncated: bool,
}

/// `FileIOService.decode` over raw bytes: `None` for a binary file.
pub fn decode_document(bytes: Vec<u8>) -> Option<String> {
    let scan = bytes.len().min(8192);
    if bytes[..scan].contains(&0) {
        return None;
    }
    String::from_utf8(bytes).ok()
}

/// The file as a document, `None` when there is no file.
pub fn read_document(path: &Path, max_bytes: u64) -> Result<Option<DocumentFile>, FsError> {
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(FsError::io(path, &e)),
    };
    let metadata = file.metadata().map_err(|e| FsError::io(path, &e))?;
    if !metadata.is_file() {
        return Err(FsError::Io(format!(
            "{} cannot be opened: it is not a file.",
            path.display()
        )));
    }
    let modified_ms = modified_ms(&metadata).map_err(|e| FsError::io(path, &e))?;
    let mut bytes = Vec::new();
    file.take(max_bytes.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|e| FsError::io(path, &e))?;
    if bytes.len() as u64 > max_bytes {
        return Ok(Some(DocumentFile {
            text: String::new(),
            is_binary: false,
            modified_ms,
            truncated: true,
        }));
    }
    let text = decode_document(bytes);
    Ok(Some(DocumentFile {
        is_binary: text.is_none(),
        text: text.unwrap_or_default(),
        modified_ms,
        truncated: false,
    }))
}

fn temp_path(path: &Path) -> PathBuf {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    path.with_file_name(format!(".{name}.{}.tmp", uuid::Uuid::new_v4().simple()))
}

/// The file a save writes: a symbolic link's target (so saving through a link updates the
/// file it names and keeps the link), otherwise `path` itself.
fn save_target(path: &Path) -> Result<PathBuf, FsError> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            pu_core::canon::canonicalize(path).map_err(|e| FsError::io(path, &e))
        }
        _ => Ok(path.to_path_buf()),
    }
}

/// Puts `temp` in place of `path`. An existing file is replaced with `ReplaceFileW`, which
/// keeps the replaced file's security descriptor (its ACL), attributes and creation time, as
/// an editor's save should; a plain rename would give the file the temporary's inherited ACL.
/// A new file is renamed into place and inherits its folder's ACL, like any new file.
fn replace_with(temp: &Path, path: &Path) -> std::io::Result<()> {
    #[cfg(windows)]
    if std::fs::symlink_metadata(path).is_ok() {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::Storage::FileSystem::{
            REPLACEFILE_IGNORE_MERGE_ERRORS, ReplaceFileW,
        };
        let wide = |p: &Path| -> Vec<u16> {
            p.as_os_str()
                .encode_wide()
                .chain(std::iter::once(0))
                .collect()
        };
        let (replaced, replacement) = (wide(path), wide(temp));
        // SAFETY: both are NUL-terminated wide strings that outlive the call; no backup file,
        // no exclude or reserved pointers.
        let ok = unsafe {
            ReplaceFileW(
                replaced.as_ptr(),
                replacement.as_ptr(),
                std::ptr::null(),
                REPLACEFILE_IGNORE_MERGE_ERRORS,
                std::ptr::null(),
                std::ptr::null(),
            )
        };
        return if ok != 0 {
            Ok(())
        } else {
            Err(std::io::Error::last_os_error())
        };
    }
    std::fs::rename(temp, path)
}

/// Write `text` unless the file changed: its modification time must still be
/// `expected_modified_ms` (`None`: there must be no file). Creates the parent directory and
/// replaces the file through a temporary sibling, so readers never see half a document.
/// Saving through a symbolic link writes its target; an existing file keeps its ACL
/// ([`replace_with`]). Returns the new modification time.
pub fn write_if_unmodified(
    path: &Path,
    text: &str,
    expected_modified_ms: Option<u64>,
) -> Result<u64, FsError> {
    let target = save_target(path)?;
    let path = target.as_path();
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
    if let Err(e) = replace_with(&temp, path) {
        let _ = std::fs::remove_file(&temp);
        return Err(FsError::io(path, &e));
    }
    stat(path)?.ok_or_else(|| FsError::Io(format!("{} vanished after writing.", path.display())))
}

pub(crate) async fn blocking<T: Send + 'static>(
    body: impl FnOnce() -> Result<T, FsError> + Send + 'static,
) -> Result<T, FsError> {
    tauri::async_runtime::spawn_blocking(body)
        .await
        .map_err(|e| FsError::Io(e.to_string()))?
}

/// `path` resolved against the open projects' roots ([`super::roots`]).
pub(crate) fn allowed(path: &str, access: Access) -> Result<PathBuf, FsError> {
    super::roots::global().check(Path::new(path), access)
}

/// `fs_stat_text(path)`: the modification time, or null when there is no file.
#[tauri::command]
pub async fn fs_stat_text(path: String) -> Result<Option<u64>, FsError> {
    blocking(move || stat(&allowed(&path, Access::Read)?)).await
}

/// `fs_read_text(path, maxBytes)`: the file's text, or null when there is no file.
#[tauri::command]
pub async fn fs_read_text(
    path: String,
    max_bytes: Option<u64>,
) -> Result<Option<TextFile>, FsError> {
    let max = max_bytes.unwrap_or(DEFAULT_MAX_BYTES);
    blocking(move || read(&allowed(&path, Access::Read)?, max)).await
}

/// `fs_read_document(path, maxBytes)`: the file for the editor, or null when there is no file.
#[tauri::command]
pub async fn fs_read_document(
    path: String,
    max_bytes: Option<u64>,
) -> Result<Option<DocumentFile>, FsError> {
    let max = max_bytes.unwrap_or(DEFAULT_MAX_BYTES);
    blocking(move || read_document(&allowed(&path, Access::Read)?, max)).await
}

/// `fs_write_text_if_unmodified(path, text, expectedModifiedMs)`: the new modification time.
#[tauri::command]
pub async fn fs_write_text_if_unmodified(
    path: String,
    text: String,
    expected_modified_ms: Option<u64>,
) -> Result<u64, FsError> {
    blocking(move || {
        write_if_unmodified(&allowed(&path, Access::Write)?, &text, expected_modified_ms)
    })
    .await
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

    #[cfg(windows)]
    #[test]
    fn given_an_explicit_acl_should_keep_it_across_saves() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.txt");
        let first = write_if_unmodified(&path, "one", None).unwrap();
        let icacls = |args: &[&str]| {
            std::process::Command::new("icacls")
                .arg(&path)
                .args(args)
                .output()
                .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        };
        // A well-known SID, so the output does not depend on the language of Windows.
        if icacls(&["/grant", "*S-1-1-0:(R)"]).is_err() {
            return;
        }
        let granted = |listing: &str| listing.contains("Everyone") || listing.contains("S-1-1-0");
        if !granted(&icacls(&[]).unwrap()) {
            return; // the grant did not take here
        }
        let modified = stat(&path).unwrap();
        assert!(modified.is_some());
        write_if_unmodified(&path, "two", modified).unwrap();
        assert_eq!(read(&path, 100).unwrap().unwrap().text, "two");
        assert!(granted(&icacls(&[]).unwrap()), "the save dropped the ACL");
        let _ = first;
    }

    #[test]
    fn given_a_symbolic_link_should_save_its_target_and_keep_the_link() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("real.txt");
        let link = dir.path().join("link.txt");
        let modified = write_if_unmodified(&target, "one", None).unwrap();
        #[cfg(windows)]
        let made = std::os::windows::fs::symlink_file(&target, &link);
        #[cfg(not(windows))]
        let made = std::os::unix::fs::symlink(&target, &link);
        if made.is_err() {
            return; // symbolic links need Developer Mode or elevation here
        }
        write_if_unmodified(&link, "two", Some(modified)).unwrap();
        assert!(
            std::fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "two");
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
    fn given_text_binary_and_long_files_should_read_documents() {
        let dir = tempfile::tempdir().unwrap();
        let text = dir.path().join("a.txt");
        std::fs::write(&text, "héllo\n").unwrap();
        let file = read_document(&text, 100).unwrap().unwrap();
        assert_eq!(file.text, "héllo\n");
        assert!(!file.is_binary && !file.truncated);
        assert_eq!(Some(file.modified_ms), stat(&text).unwrap());

        let nul = dir.path().join("b.bin");
        std::fs::write(&nul, b"ab\0cd").unwrap();
        let file = read_document(&nul, 100).unwrap().unwrap();
        assert!(file.is_binary);
        assert_eq!(file.text, "");

        let latin1 = dir.path().join("c.txt");
        std::fs::write(&latin1, [0x63, 0x61, 0x66, 0xe9]).unwrap();
        assert!(read_document(&latin1, 100).unwrap().unwrap().is_binary);

        let long = read_document(&text, 3).unwrap().unwrap();
        assert!(long.truncated);
        assert_eq!(read_document(&dir.path().join("none"), 3), Ok(None));
        assert!(matches!(read_document(dir.path(), 3), Err(FsError::Io(_))));
    }

    #[test]
    fn given_document_should_serialize_camel_case() {
        let json = serde_json::to_value(DocumentFile {
            text: "t".into(),
            is_binary: false,
            modified_ms: 3,
            truncated: false,
        })
        .unwrap();
        assert_eq!(
            json,
            serde_json::json!({ "text": "t", "isBinary": false, "modifiedMs": 3, "truncated": false })
        );
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
