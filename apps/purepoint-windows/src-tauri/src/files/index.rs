//! Port of `Services/FileIndex.swift` (WIN-6.6): a flat, bounded listing of a directory's
//! files for palette search. Skips the same noise directories as the file tree so results
//! match what the tree shows.
//!
//! Like `FileManager.enumerator` on the Mac, the walk is depth-first and pre-order, includes
//! hidden files, and does not follow symbolic links (a link is listed as a file). Entries are
//! visited in name order so the listing is stable. Relative paths use `/` on every platform,
//! as the palette's search text; absolute paths are native.

use std::path::{Path, PathBuf};

use serde::Serialize;

/// The most files listed when the caller gives no limit.
pub const DEFAULT_LIMIT: usize = 5000;

/// Names skipped wherever they appear, with everything beneath them. Kept in step with
/// `FILE_INDEX_SKIPPED_NAMES` in `ui/src/services/fileIndex.ts`.
pub const SKIPPED_NAMES: [&str; 10] = [
    ".git",
    ".DS_Store",
    ".build",
    ".swiftpm",
    "xcuserdata",
    "DerivedData",
    "__pycache__",
    ".tsbuildinfo",
    "node_modules",
    "target",
];

/// A file the palette can open into a file pane (`PaletteFileEntry` in the front end).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PaletteFileEntry {
    pub relative_path: String,
    pub absolute_path: String,
}

/// A walk entry: path relative to the root (`/`-separated), absolute path, is a directory.
type Node = (String, PathBuf, bool);

/// Pushes the unskipped children of `dir` so they pop in name order.
fn push_children(stack: &mut Vec<Node>, prefix: &str, dir: &Path) {
    let Ok(read) = std::fs::read_dir(dir) else {
        return;
    };
    let mut children: Vec<(String, PathBuf, bool)> = read
        .filter_map(Result::ok)
        .map(|entry| {
            let is_dir = entry.file_type().is_ok_and(|t| t.is_dir());
            let name = entry.file_name().to_string_lossy().into_owned();
            (name, entry.path(), is_dir)
        })
        .filter(|(name, _, _)| !SKIPPED_NAMES.contains(&name.as_str()))
        .collect();
    children.sort_by(|a, b| b.0.cmp(&a.0));
    for (name, path, is_dir) in children {
        let relative = if prefix.is_empty() {
            name
        } else {
            format!("{prefix}/{name}")
        };
        stack.push((relative, path, is_dir));
    }
}

/// Up to `limit` files under `root`. A missing or unreadable root lists nothing.
pub fn list(root: &Path, limit: usize) -> Vec<PaletteFileEntry> {
    let mut entries = Vec::new();
    if limit == 0 || !root.is_dir() {
        return entries;
    }
    let mut stack = Vec::new();
    push_children(&mut stack, "", root);
    while let Some((relative, path, is_dir)) = stack.pop() {
        if is_dir {
            push_children(&mut stack, &relative, &path);
            continue;
        }
        entries.push(PaletteFileEntry {
            relative_path: relative,
            absolute_path: path.to_string_lossy().into_owned(),
        });
        if entries.len() >= limit {
            break;
        }
    }
    entries
}

/// `file_index(root, limit)`: [`list`] off the main thread; `limit` defaults to
/// [`DEFAULT_LIMIT`].
#[tauri::command]
pub async fn file_index(
    root: String,
    limit: Option<usize>,
) -> Result<Vec<PaletteFileEntry>, String> {
    let limit = limit.unwrap_or(DEFAULT_LIMIT);
    tauri::async_runtime::spawn_blocking(move || {
        super::roots::global()
            .check(Path::new(&root), super::roots::Access::Read)
            .map_err(|e| match e {
                super::text::FsError::Conflict(m) | super::text::FsError::Io(m) => m,
            })?;
        Ok(list(Path::new(&root), limit))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// A fresh directory under the system temp dir, removed on drop.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let path = std::env::temp_dir().join(format!(
                "pp-file-index-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).unwrap();
            TempDir(path)
        }

        fn file(&self, relative: &str) -> &Self {
            let path = self.0.join(relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, b"x").unwrap();
            self
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn relative_paths(entries: &[PaletteFileEntry]) -> Vec<&str> {
        entries.iter().map(|e| e.relative_path.as_str()).collect()
    }

    #[test]
    fn lists_files_recursively_depth_first_in_name_order() {
        let dir = TempDir::new();
        dir.file("b.txt")
            .file("a/z.rs")
            .file("a/deep/y.md")
            .file("c/x.ts");
        let entries = list(&dir.0, DEFAULT_LIMIT);
        assert_eq!(
            relative_paths(&entries),
            ["a/deep/y.md", "a/z.rs", "b.txt", "c/x.ts"]
        );
    }

    #[test]
    fn directories_are_not_entries() {
        let dir = TempDir::new();
        fs::create_dir_all(dir.0.join("empty/nested")).unwrap();
        dir.file("only.txt");
        assert_eq!(relative_paths(&list(&dir.0, DEFAULT_LIMIT)), ["only.txt"]);
    }

    #[test]
    fn absolute_path_is_the_file_under_root() {
        let dir = TempDir::new();
        dir.file("src/main.rs");
        let entries = list(&dir.0, DEFAULT_LIMIT);
        assert_eq!(entries.len(), 1);
        assert_eq!(
            PathBuf::from(&entries[0].absolute_path),
            dir.0.join("src").join("main.rs")
        );
    }

    #[test]
    fn skips_noise_directories_and_files_at_any_depth() {
        let dir = TempDir::new();
        for name in SKIPPED_NAMES {
            dir.file(&format!("{name}/inside.txt"));
            dir.file(&format!("pkg/{name}/inside.txt"));
        }
        dir.file("pkg/kept.txt");
        fs::remove_dir_all(dir.0.join(".DS_Store")).unwrap();
        dir.file(".DS_Store");
        assert_eq!(
            relative_paths(&list(&dir.0, DEFAULT_LIMIT)),
            ["pkg/kept.txt"]
        );
    }

    #[test]
    fn includes_hidden_files_that_are_not_skipped() {
        let dir = TempDir::new();
        dir.file(".gitignore").file(".github/workflows/ci.yml");
        assert_eq!(
            relative_paths(&list(&dir.0, DEFAULT_LIMIT)),
            [".github/workflows/ci.yml", ".gitignore"]
        );
    }

    #[test]
    fn stops_at_the_limit() {
        let dir = TempDir::new();
        for i in 0..10 {
            dir.file(&format!("f{i}.txt"));
        }
        assert_eq!(
            relative_paths(&list(&dir.0, 3)),
            ["f0.txt", "f1.txt", "f2.txt"]
        );
        assert_eq!(list(&dir.0, 10).len(), 10);
        assert_eq!(list(&dir.0, 50).len(), 10);
        assert!(list(&dir.0, 0).is_empty());
    }

    #[test]
    fn missing_or_file_root_lists_nothing() {
        let dir = TempDir::new();
        dir.file("a.txt");
        assert!(list(&dir.0.join("missing"), DEFAULT_LIMIT).is_empty());
        assert!(list(&dir.0.join("a.txt"), DEFAULT_LIMIT).is_empty());
    }

    #[test]
    fn default_limit_matches_the_mac() {
        assert_eq!(DEFAULT_LIMIT, 5000);
    }

    #[test]
    fn entries_serialize_in_camel_case_for_the_front_end() {
        let entry = PaletteFileEntry {
            relative_path: "a/b.rs".into(),
            absolute_path: "C:/p/a/b.rs".into(),
        };
        assert_eq!(
            serde_json::to_string(&entry).unwrap(),
            r#"{"relativePath":"a/b.rs","absolutePath":"C:/p/a/b.rs"}"#
        );
    }
}
