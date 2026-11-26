//! The file tree's local directory access (WIN-8.4): one directory's entries with their
//! `git check-ignore` verdicts, and the tree's Rename and Delete.
//!
//! Port of `FileTreeState.scanDirectory` and `computeGitIgnored` from
//! `State/FileTreeState.swift`. The front end filters the hidden names and the ignored
//! entries, exactly as a remote tree filters the daemon's `fs_list` reply, so both hosts
//! share one filter. Like the Mac's `fileExists(atPath:isDirectory:)`, a symbolic link is
//! classified by its target and a broken link is skipped. Dot names are never checked
//! against `.gitignore` (the Mac shows `.env` even when it is ignored).

use std::collections::HashSet;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::Serialize;

use super::roots::Access;
use super::text::{FsError, allowed, blocking};
use crate::git::runner::{git_path, hide_console};

/// One entry of a directory listing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DirEntry {
    pub name: String,
    pub path: String,
    pub is_directory: bool,
    /// `git check-ignore` reports the entry as ignored (never set for dot names).
    pub ignored: bool,
}

fn io_error(path: &Path, error: &std::io::Error) -> FsError {
    FsError::Io(format!("{}: {error}", path.display()))
}

/// `relative` (from the root, `/`-separated) of every non-dot entry, for `git check-ignore`.
fn relative_path(root: &Path, path: &Path) -> Option<String> {
    let relative = path.strip_prefix(root).ok()?;
    let parts: Vec<String> = relative
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    (!parts.is_empty()).then(|| parts.join("/"))
}

/// The paths among `relative` that git ignores in the repository at `root`; empty when
/// `root` is not a repository or git cannot run (the Mac's `try?`).
fn git_ignored(root: &Path, relative: &[String]) -> HashSet<String> {
    if relative.is_empty() {
        return HashSet::new();
    }
    let mut command = Command::new(git_path());
    command
        .arg("-C")
        .arg(root)
        .args(["check-ignore", "--stdin"])
        .current_dir(root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    hide_console(&mut command);
    let Ok(mut child) = command.spawn() else {
        return HashSet::new();
    };
    if let Some(mut stdin) = child.stdin.take() {
        let input = relative.join("\n");
        let _ = stdin.write_all(input.as_bytes());
        // Dropping stdin closes it, so git sees the end of the list.
    }
    let Ok(output) = child.wait_with_output() else {
        return HashSet::new();
    };
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|line| line.trim_end_matches('\r').to_string())
        .filter(|line| !line.is_empty())
        .collect()
}

/// The entries of `dir` (unsorted), each with its ignore verdict for the repository at `root`.
pub fn list(dir: &Path, root: &Path) -> Result<Vec<DirEntry>, FsError> {
    let read = std::fs::read_dir(dir).map_err(|e| io_error(dir, &e))?;
    let mut entries: Vec<(String, PathBuf, bool)> = Vec::new();
    for entry in read.filter_map(Result::ok) {
        let name = entry.file_name().to_string_lossy().into_owned();
        let path = entry.path();
        // Follows links: a link to a directory expands, a broken link is skipped.
        let Ok(metadata) = std::fs::metadata(&path) else {
            continue;
        };
        entries.push((name, path, metadata.is_dir()));
    }
    let candidates: Vec<String> = entries
        .iter()
        .filter(|(name, _, _)| !name.starts_with('.'))
        .filter_map(|(_, path, _)| relative_path(root, path))
        .collect();
    let ignored = git_ignored(root, &candidates);
    Ok(entries
        .into_iter()
        .map(|(name, path, is_directory)| {
            let is_ignored = !name.starts_with('.')
                && relative_path(root, &path).is_some_and(|r| ignored.contains(&r));
            DirEntry {
                name,
                path: path.to_string_lossy().into_owned(),
                is_directory,
                ignored: is_ignored,
            }
        })
        .collect())
}

/// Renames `from` to `to`, refusing to replace anything already at `to`.
pub fn rename(from: &Path, to: &Path) -> Result<(), FsError> {
    if std::fs::symlink_metadata(to).is_ok() {
        return Err(FsError::Io(format!("{} already exists.", to.display())));
    }
    std::fs::rename(from, to).map_err(|e| io_error(from, &e))
}

/// Deletes a file, or a directory with everything in it.
pub fn delete(path: &Path) -> Result<(), FsError> {
    let metadata = std::fs::symlink_metadata(path).map_err(|e| io_error(path, &e))?;
    if metadata.is_dir() {
        std::fs::remove_dir_all(path).map_err(|e| io_error(path, &e))
    } else {
        std::fs::remove_file(path).map_err(|e| io_error(path, &e))
    }
}

/// `fs_list_dir(path, root)`: the entries of `path`, with `git check-ignore` run in `root`.
#[tauri::command]
pub async fn fs_list_dir(path: String, root: String) -> Result<Vec<DirEntry>, FsError> {
    blocking(move || {
        allowed(&path, Access::Read)?;
        allowed(&root, Access::Read)?;
        // Listed under the caller's spelling, so a linked folder's entries keep their place in
        // the tree; the check above resolved the link and found its target inside a project.
        list(Path::new(&path), Path::new(&root))
    })
    .await
}

/// `fs_rename(from, to)`: the file tree's Rename.
#[tauri::command]
pub async fn fs_rename(from: String, to: String) -> Result<(), FsError> {
    blocking(move || {
        rename(
            &allowed(&from, Access::Entry)?,
            &allowed(&to, Access::Create)?,
        )
    })
    .await
}

/// `fs_delete(path)`: the file tree's Delete.
#[tauri::command]
pub async fn fs_delete(path: String) -> Result<(), FsError> {
    blocking(move || delete(&allowed(&path, Access::Entry)?)).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(entries: &[DirEntry]) -> Vec<String> {
        let mut names: Vec<String> = entries.iter().map(|e| e.name.clone()).collect();
        names.sort();
        names
    }

    fn git(root: &Path, args: &[&str]) -> bool {
        let mut command = Command::new(git_path());
        command.arg("-C").arg(root).args(args);
        hide_console(&mut command);
        command
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    }

    #[test]
    fn given_a_directory_should_list_files_and_folders() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "a").unwrap();
        std::fs::write(dir.path().join(".env"), "x").unwrap();
        std::fs::create_dir(dir.path().join("src")).unwrap();
        let entries = list(dir.path(), dir.path()).unwrap();
        assert_eq!(names(&entries), vec![".env", "a.txt", "src"]);
        let src = entries.iter().find(|e| e.name == "src").unwrap();
        assert!(src.is_directory);
        assert_eq!(Path::new(&src.path), dir.path().join("src"));
        // Not a repository: nothing is ignored.
        assert!(entries.iter().all(|e| !e.ignored));
    }

    #[test]
    fn given_a_repository_should_mark_ignored_entries_but_never_dot_names() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        if !git(root, &["init", "-q"]) {
            return; // git is not installed here
        }
        std::fs::write(root.join(".gitignore"), "*.ignore\n.secret\n").unwrap();
        std::fs::write(root.join("root.ignore"), "").unwrap();
        std::fs::write(root.join("keep.txt"), "").unwrap();
        std::fs::write(root.join(".secret"), "").unwrap();
        std::fs::create_dir(root.join("Sources")).unwrap();
        std::fs::write(root.join("Sources").join("child.ignore"), "").unwrap();
        std::fs::write(root.join("Sources").join("keep.rs"), "").unwrap();

        let top = list(root, root).unwrap();
        let ignored: Vec<&str> = top
            .iter()
            .filter(|e| e.ignored)
            .map(|e| e.name.as_str())
            .collect();
        assert_eq!(ignored, vec!["root.ignore"]);

        let nested = list(&root.join("Sources"), root).unwrap();
        let ignored: Vec<&str> = nested
            .iter()
            .filter(|e| e.ignored)
            .map(|e| e.name.as_str())
            .collect();
        assert_eq!(ignored, vec!["child.ignore"]);
    }

    #[test]
    fn given_missing_directory_should_fail() {
        let dir = tempfile::tempdir().unwrap();
        assert!(matches!(
            list(&dir.path().join("gone"), dir.path()),
            Err(FsError::Io(_))
        ));
    }

    #[test]
    fn given_rename_should_move_and_refuse_to_replace() {
        let dir = tempfile::tempdir().unwrap();
        let from = dir.path().join("a.txt");
        let to = dir.path().join("b.txt");
        std::fs::write(&from, "a").unwrap();
        rename(&from, &to).unwrap();
        assert!(!from.exists());
        assert_eq!(std::fs::read_to_string(&to).unwrap(), "a");
        std::fs::write(&from, "new").unwrap();
        assert!(matches!(rename(&from, &to), Err(FsError::Io(_))));
        assert_eq!(std::fs::read_to_string(&to).unwrap(), "a");
    }

    #[test]
    fn given_delete_should_remove_files_and_folders() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.txt");
        let folder = dir.path().join("src");
        std::fs::write(&file, "a").unwrap();
        std::fs::create_dir(&folder).unwrap();
        std::fs::write(folder.join("b.txt"), "b").unwrap();
        delete(&file).unwrap();
        delete(&folder).unwrap();
        assert!(!file.exists());
        assert!(!folder.exists());
        assert!(matches!(delete(&file), Err(FsError::Io(_))));
    }

    #[test]
    fn given_entry_should_serialize_camel_case() {
        let json = serde_json::to_value(DirEntry {
            name: "a".into(),
            path: "C:\\a".into(),
            is_directory: true,
            ignored: false,
        })
        .unwrap();
        assert_eq!(
            json,
            serde_json::json!({ "name": "a", "path": "C:\\a", "isDirectory": true, "ignored": false })
        );
    }
}
