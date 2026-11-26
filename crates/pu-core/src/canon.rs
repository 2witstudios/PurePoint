//! The one canonicalization helper (WINAPP-014, WIN-3.11).
//!
//! Paths the engine and CLI store or send (manifests, `.projects.json`,
//! protocol payloads) are shared with other clients and must not drift.
//! `std::fs::canonicalize` returns `\\?\C:\...` on Windows, so every caller
//! goes through [`canonicalize`] (enforced by `disallowed-methods` in
//! `clippy.toml`). On Unix it is exactly `std::fs::canonicalize`.

use std::io;
use std::path::{Path, PathBuf};

/// `dunce::canonicalize`, then [`normalize`]: on Windows no `\\?\` prefix,
/// `\` separators, an uppercase drive letter and no trailing separator, with
/// the on-disk casing of every component.
pub fn canonicalize(path: impl AsRef<Path>) -> io::Result<PathBuf> {
    dunce::canonicalize(path).map(|p| normalize(&p))
}

/// Method form of [`canonicalize`], replacing `Path::canonicalize`.
pub trait CanonicalPath {
    fn canonical(&self) -> io::Result<PathBuf>;
}

impl CanonicalPath for Path {
    fn canonical(&self) -> io::Result<PathBuf> {
        canonicalize(self)
    }
}

/// The stored spelling of a path that may not exist (so cannot be
/// canonicalized). Windows: `\\?\` stripped where safe, `/` → `\`, uppercase
/// drive letter, no trailing separator except at a root; never lowercased.
/// Unix: unchanged.
pub fn normalize(path: &Path) -> PathBuf {
    if !cfg!(windows) {
        return path.to_path_buf();
    }
    let text = PathBuf::from(path.to_string_lossy().replace('/', "\\"));
    let text = dunce::simplified(&text).to_string_lossy().into_owned();
    let mut text = text;
    if text.as_bytes().get(1) == Some(&b':') && text.as_bytes()[0].is_ascii_alphabetic() {
        text[..1].make_ascii_uppercase();
    }
    while text.len() > 1 && text.ends_with('\\') && !text.ends_with(":\\") && text != "\\\\" {
        text.pop();
    }
    PathBuf::from(text)
}

/// Identity of a project or worktree path: one normalized form, compared
/// case-insensitively on Windows (NTFS directories are case-insensitive by
/// default) and byte-exactly on Unix.
pub fn same_path(a: impl AsRef<Path>, b: impl AsRef<Path>) -> bool {
    let (a, b) = (a.as_ref(), b.as_ref());
    let (na, nb) = (normalize(a), normalize(b));
    if cfg!(windows) {
        let fold = |p: &Path| p.to_string_lossy().to_lowercase();
        // Same spelling, or the same existing directory under another one
        // (8.3 short names such as RUNNER~1, junctions): the canonical forms
        // decide.
        fold(&na) == fold(&nb)
            || matches!((canonicalize(a), canonicalize(b)), (Ok(ca), Ok(cb)) if fold(&ca) == fold(&cb))
    } else {
        na == nb
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_existing_dir_should_canonicalize_without_verbatim_prefix() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("Project");
        std::fs::create_dir(&dir).unwrap();
        let canonical = dir.canonical().unwrap();
        assert!(
            !canonical.to_string_lossy().starts_with(r"\\?\"),
            "{canonical:?}"
        );
        assert!(canonical.is_absolute());
        assert_eq!(canonicalize(&dir).unwrap(), canonical);
        assert!(same_path(&canonical, &dir));
    }

    #[cfg(unix)]
    #[test]
    fn given_unix_paths_should_compare_exactly() {
        assert!(same_path("/a/b", "/a/b"));
        assert!(!same_path("/a/B", "/a/b"));
        assert_eq!(normalize(Path::new("/a/b/")), PathBuf::from("/a/b/"));
    }

    #[cfg(windows)]
    #[test]
    fn given_windows_spellings_should_share_one_identity() {
        assert_eq!(
            normalize(Path::new(r"\\?\c:/Users/Me/Project\")),
            PathBuf::from(r"C:\Users\Me\Project")
        );
        assert_eq!(normalize(Path::new(r"c:\")), PathBuf::from(r"C:\"));
        assert!(same_path(r"c:/users/me/PROJECT/", r"C:\Users\Me\Project"));
        assert!(!same_path(r"C:\Users\Me\Project2", r"C:\Users\Me\Project"));
    }

    #[cfg(windows)]
    #[test]
    fn given_other_case_spelling_should_canonicalize_to_on_disk_casing() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("MixedCase");
        std::fs::create_dir(&dir).unwrap();
        let canonical = dir.canonical().unwrap();
        let other = canonical
            .to_string_lossy()
            .to_lowercase()
            .replace('\\', "/");
        let respelled = canonicalize(format!("{other}/")).unwrap();
        assert_eq!(respelled, canonical);
        assert!(respelled.ends_with("MixedCase"));
        let text = respelled.to_string_lossy();
        assert!(text.as_bytes()[0].is_ascii_uppercase(), "{text}");
    }
}
