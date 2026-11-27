//! The folders the local file commands may touch: the open projects' roots.
//!
//! The webview is not trusted with arbitrary paths. Every `fs_*` command (and the default-app
//! opener, `opener.rs`) resolves its path here first: the path is canonicalized
//! (`pu_core::canon`, so `..`, 8.3 names, junctions and symbolic links are resolved) and must
//! lie at or under an allowed root, compared case-insensitively on Windows. Mutations (write,
//! rename, delete) also refuse a reparse point between the root and the target, act on the
//! resolved path rather than the caller's spelling, and Delete never targets a root itself.
//!
//! Roots come from the backend's own view of the open projects: the daemon's registered
//! projects (the projects subscription, recorded before the front end hears of them), and the
//! roots the front end starts subscriptions or watchers on. A folder that could hold every
//! project (a drive or share root, the user profile or one of its ancestors) is never a root.
//! A project's `.pu\workspaces.json` (the layout document) and its worktrees live under its
//! root, so they are covered by it.

use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use super::text::FsError;

/// What the caller is about to do with the path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    /// Read, list or stat; links are followed, the target must be inside a root.
    Read,
    /// Replace a file's contents; a link to a file is followed to its target (the save writes
    /// the target), and the parent directories may not exist yet.
    Write,
    /// Rename's source or Delete's target: the entry itself, never what a link points to.
    Entry,
    /// Rename's destination, which must not exist yet.
    Create,
}

/// The allowed roots: the daemon's projects and the roots the front end opened.
#[derive(Debug, Default)]
pub struct AllowedRoots {
    inner: Mutex<Sets>,
}

#[derive(Debug, Default)]
struct Sets {
    daemon: Vec<PathBuf>,
    opened: Vec<PathBuf>,
}

/// The app's roots, shared by the file commands, subscriptions and watchers.
pub fn global() -> &'static AllowedRoots {
    static ROOTS: OnceLock<AllowedRoots> = OnceLock::new();
    ROOTS.get_or_init(AllowedRoots::default)
}

fn denied(path: &Path, why: &str) -> FsError {
    FsError::Io(format!("{} cannot be used: {why}", path.display()))
}

/// Components compared as Windows compares names: case-insensitively.
fn key(component: &std::ffi::OsStr) -> String {
    let text = component.to_string_lossy();
    if cfg!(windows) {
        text.to_lowercase()
    } else {
        text.into_owned()
    }
}

/// `path` is `root` or under it, component by component.
pub fn is_within(path: &Path, root: &Path) -> bool {
    let mut path = path.components();
    for part in root.components() {
        match path.next() {
            Some(other) if key(other.as_os_str()) == key(part.as_os_str()) => {}
            _ => return false,
        }
    }
    true
}

fn same(a: &Path, b: &Path) -> bool {
    is_within(a, b) && is_within(b, a)
}

/// The canonical form of `path`, which need not exist: the deepest existing ancestor is
/// canonicalized and the missing names appended. A `..` among the missing names is refused,
/// since it cannot be resolved against the disk.
pub fn canonical_lenient(path: &Path) -> Result<PathBuf, FsError> {
    if !path.is_absolute() {
        return Err(denied(path, "the path is not absolute."));
    }
    let mut existing = path.to_path_buf();
    let mut missing: Vec<OsString> = Vec::new();
    loop {
        match pu_core::canon::canonicalize(&existing) {
            Ok(mut canonical) => {
                for name in missing.iter().rev() {
                    canonical.push(name);
                }
                return Ok(canonical);
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(FsError::Io(format!("{}: {error}", path.display()))),
        }
        match existing.components().next_back() {
            Some(Component::Normal(name)) => missing.push(name.to_os_string()),
            Some(Component::CurDir) => {}
            _ => return Err(denied(path, "the path cannot be resolved.")),
        }
        if !existing.pop() {
            return Err(denied(path, "the path cannot be resolved."));
        }
    }
}

/// The entry itself: its canonical parent joined with its own name, so a link is not followed.
fn canonical_entry(path: &Path) -> Result<PathBuf, FsError> {
    let Some(Component::Normal(name)) = path.components().next_back() else {
        return Err(denied(path, "the path names no file or folder."));
    };
    let parent = path
        .parent()
        .ok_or_else(|| denied(path, "the path names no file or folder."))?;
    Ok(canonical_lenient(parent)?.join(name))
}

/// The entry at `path` is a reparse point (a symbolic link, junction or mount point).
pub fn is_reparse_point(path: &Path) -> bool {
    let Ok(metadata) = std::fs::symlink_metadata(path) else {
        return false;
    };
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
        metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    }
    #[cfg(not(windows))]
    {
        metadata.file_type().is_symlink()
    }
}

/// Folders that could hold every project: a drive or share root, the user profile and its
/// ancestors.
fn too_broad(root: &Path) -> bool {
    if root.parent().is_none() || root.components().count() <= 1 {
        return true;
    }
    if let Some(Component::Prefix(_)) = root.components().next()
        && root.components().count() <= 2
    {
        return true;
    }
    let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME"));
    home.and_then(|home| pu_core::canon::canonicalize(home).ok())
        .is_some_and(|home| is_within(&home, root))
}

fn root_form(root: &Path) -> Option<PathBuf> {
    if !root.is_absolute() {
        return None;
    }
    let canonical =
        pu_core::canon::canonicalize(root).unwrap_or_else(|_| pu_core::canon::normalize(root));
    (!too_broad(&canonical)).then_some(canonical)
}

impl AllowedRoots {
    fn sets(&self) -> std::sync::MutexGuard<'_, Sets> {
        self.inner.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Allows `root` (a project or worktree the front end opened). False when it is refused
    /// as too broad or not absolute.
    pub fn add(&self, root: impl AsRef<Path>) -> bool {
        let Some(root) = root_form(root.as_ref()) else {
            return false;
        };
        let mut sets = self.sets();
        if !sets.opened.iter().any(|r| same(r, &root)) {
            sets.opened.push(root);
        }
        true
    }

    /// Replaces the daemon's registered projects (each `projects_changed` reply).
    pub fn set_daemon_projects<S: AsRef<Path>>(&self, roots: &[S]) {
        let roots: Vec<PathBuf> = roots.iter().filter_map(|r| root_form(r.as_ref())).collect();
        self.sets().daemon = roots;
    }

    /// Forgets every root (tests).
    pub fn clear(&self) {
        *self.sets() = Sets::default();
    }

    fn roots(&self) -> Vec<PathBuf> {
        let sets = self.sets();
        sets.daemon.iter().chain(&sets.opened).cloned().collect()
    }

    /// The resolved path to act on, or an error when `path` is outside every root (or, for a
    /// mutation, crosses a reparse point or targets a root).
    pub fn check(&self, path: &Path, access: Access) -> Result<PathBuf, FsError> {
        let resolved = match access {
            Access::Read => canonical_lenient(path)?,
            Access::Entry | Access::Create => canonical_entry(path)?,
            Access::Write => {
                let entry = canonical_entry(path)?;
                if is_reparse_point(&entry) {
                    // A link to a file: save into its target, which must be inside a root too.
                    pu_core::canon::canonicalize(&entry)
                        .map_err(|e| FsError::Io(format!("{}: {e}", path.display())))?
                } else {
                    entry
                }
            }
        };
        let roots = self.roots();
        let Some(root) = roots
            .iter()
            .filter(|root| is_within(&resolved, root))
            .max_by_key(|root| root.components().count())
        else {
            return Err(denied(path, "it is outside the open projects."));
        };
        if access == Access::Read {
            return Ok(resolved);
        }
        if access == Access::Entry && roots.iter().any(|r| same(r, &resolved)) {
            return Err(denied(path, "it is a project root."));
        }
        // Every existing folder between the root and the target (and the target itself, for
        // Rename and Delete) must be a plain folder: a junction swapped in after the check
        // could otherwise redirect the change outside the project.
        let below: Vec<Component<'_>> = resolved
            .components()
            .skip(root.components().count())
            .collect();
        let mut current = root.clone();
        for (index, part) in below.iter().enumerate() {
            current.push(part);
            let last = index + 1 == below.len();
            if last && access != Access::Entry {
                break;
            }
            if is_reparse_point(&current) {
                return Err(denied(path, "it crosses a link or junction."));
            }
        }
        Ok(resolved)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture {
        _dir: tempfile::TempDir,
        root: PathBuf,
        outside: PathBuf,
        roots: AllowedRoots,
    }

    fn fixture() -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let base = pu_core::canon::canonicalize(dir.path()).unwrap();
        let root = base.join("Project");
        let outside = base.join("Outside");
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(root.join("src").join("a.txt"), "a").unwrap();
        std::fs::write(outside.join("secret.txt"), "s").unwrap();
        let roots = AllowedRoots::default();
        assert!(roots.add(&root));
        Fixture {
            _dir: dir,
            root,
            outside,
            roots,
        }
    }

    #[cfg(windows)]
    fn junction(link: &Path, target: &Path) -> bool {
        std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(link)
            .arg(target)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    }

    #[test]
    fn given_paths_inside_a_root_should_allow_them() {
        let f = fixture();
        let file = f.root.join("src").join("a.txt");
        assert_eq!(f.roots.check(&file, Access::Read).unwrap(), file);
        assert_eq!(f.roots.check(&file, Access::Write).unwrap(), file);
        let layout = f.root.join(".pu").join("workspaces.json");
        assert_eq!(f.roots.check(&layout, Access::Write).unwrap(), layout);
        assert_eq!(f.roots.check(&layout, Access::Read).unwrap(), layout);
        assert!(f.roots.check(&f.root, Access::Read).is_ok());
    }

    #[cfg(windows)]
    #[test]
    fn given_other_casing_should_match_the_root() {
        let f = fixture();
        let upper = PathBuf::from(f.root.to_string_lossy().to_uppercase()).join("src");
        assert!(f.roots.check(&upper, Access::Read).is_ok());
    }

    #[test]
    fn given_traversal_should_refuse_it() {
        let f = fixture();
        let escape = f
            .root
            .join("src")
            .join("..")
            .join("..")
            .join("Outside")
            .join("secret.txt");
        for access in [Access::Read, Access::Write, Access::Entry, Access::Create] {
            assert!(f.roots.check(&escape, access).is_err(), "{access:?}");
        }
        let missing = f
            .root
            .join("new")
            .join("..")
            .join("..")
            .join("Outside")
            .join("x");
        assert!(f.roots.check(&missing, Access::Write).is_err());
    }

    #[test]
    fn given_absolute_outside_or_relative_paths_should_refuse_them() {
        let f = fixture();
        assert!(
            f.roots
                .check(&f.outside.join("secret.txt"), Access::Read)
                .is_err()
        );
        assert!(f.roots.check(&f.outside, Access::Entry).is_err());
        assert!(f.roots.check(Path::new("src/a.txt"), Access::Read).is_err());
        let sibling = PathBuf::from(format!("{}-other", f.root.display()));
        std::fs::create_dir_all(&sibling).unwrap();
        assert!(f.roots.check(&sibling, Access::Read).is_err());
        assert!(
            AllowedRoots::default()
                .check(&f.root, Access::Read)
                .is_err()
        );
    }

    #[test]
    fn given_a_root_should_refuse_to_delete_it() {
        let f = fixture();
        assert!(f.roots.check(&f.root, Access::Entry).is_err());
        assert!(f.roots.check(&f.root.join("src"), Access::Entry).is_ok());
    }

    #[test]
    fn given_too_broad_roots_should_refuse_to_add_them() {
        let roots = AllowedRoots::default();
        if cfg!(windows) {
            assert!(!roots.add(r"C:\"));
        } else {
            assert!(!roots.add("/"));
        }
        if let Some(home) = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")) {
            assert!(!roots.add(&home));
            assert!(!roots.add(Path::new(&home).parent().unwrap()));
        }
        assert!(!roots.add("relative"));
    }

    #[test]
    fn given_daemon_projects_should_replace_them() {
        let f = fixture();
        let roots = AllowedRoots::default();
        roots.set_daemon_projects(&[&f.root]);
        assert!(roots.check(&f.root.join("src"), Access::Read).is_ok());
        roots.set_daemon_projects::<&Path>(&[]);
        assert!(roots.check(&f.root.join("src"), Access::Read).is_err());
    }

    #[cfg(windows)]
    #[test]
    fn given_a_junction_out_of_the_root_should_refuse_it() {
        let f = fixture();
        let link = f.root.join("escape");
        if !junction(&link, &f.outside) {
            return; // mklink is unavailable here
        }
        let through = link.join("secret.txt");
        for access in [Access::Read, Access::Write] {
            assert!(f.roots.check(&through, access).is_err(), "{access:?}");
        }
        assert!(
            f.roots
                .check(&link.join("new.txt"), Access::Create)
                .is_err()
        );
        // The junction entry itself is inside but is a reparse point: no Delete or Rename.
        assert!(f.roots.check(&link, Access::Entry).is_err());
    }

    #[cfg(windows)]
    #[test]
    fn given_a_junction_inside_the_root_should_refuse_mutations_through_it() {
        let f = fixture();
        let link = f.root.join("alias");
        if !junction(&link, &f.root.join("src")) {
            return;
        }
        // Reads resolve through it; the resolved path is a plain folder.
        assert!(f.roots.check(&link.join("a.txt"), Access::Read).is_ok());
        assert!(f.roots.check(&link, Access::Entry).is_err());
    }
}
