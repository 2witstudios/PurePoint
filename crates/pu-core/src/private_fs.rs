//! Owner-only files and directories on every platform.
//!
//! Unix: directories are mode 0700, files 0600, and [`check_private`] requires
//! the caller's uid as owner with no group/other bits.
//!
//! Windows: every created path gets a protected DACL that grants full control
//! to the current user and SYSTEM and read access to Administrators, with no
//! inherited entries. [`check_private`] requires the owner to be the current
//! user (or SYSTEM/Administrators, which already have full control of the
//! machine) and rejects any allow entry for another principal.

use std::fs::{File, OpenOptions};
use std::io;
use std::path::Path;

/// Create `dir` and any missing parents; force `dir` itself to be private.
pub fn create_private_dir(dir: &Path) -> io::Result<()> {
    imp::create_private_dir(dir)
}

/// Create a new private file, failing if it exists (`O_CREAT|O_EXCL`).
pub fn create_new_private_file(path: &Path) -> io::Result<File> {
    let mut opts = OpenOptions::new();
    opts.write(true).create_new(true);
    imp::open_private(path, &mut opts)
}

/// Open (creating if missing, never truncating) a private file for writing,
/// e.g. a lock file.
pub fn open_private_file(path: &Path) -> io::Result<File> {
    let mut opts = OpenOptions::new();
    opts.write(true).create(true).truncate(false);
    imp::open_private(path, &mut opts)
}

/// Atomically replace `path` with `contents` in a private file.
pub fn write_private_file(path: &Path, contents: &[u8]) -> io::Result<()> {
    use std::io::Write;
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "file".into());
    let temp = parent.join(format!(".{name}.tmp.{}", uuid::Uuid::new_v4()));
    let write = || -> io::Result<()> {
        let mut file = create_new_private_file(&temp)?;
        file.write_all(contents)?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&temp, path)?;
        sync_dir(parent)
    };
    write().inspect_err(|_| {
        let _ = std::fs::remove_file(&temp);
    })
}

/// Flush a directory entry change (rename/create) to disk. Unix fsyncs the
/// directory; Windows cannot open directories as files and NTFS journals
/// metadata, so it is a no-op there.
pub fn sync_dir(dir: &Path) -> io::Result<()> {
    #[cfg(unix)]
    File::open(dir)?.sync_all()?;
    #[cfg(not(unix))]
    let _ = dir;
    Ok(())
}

/// Ok when `path` is owned by the current user and no one else can access it.
pub fn check_private(path: &Path) -> io::Result<()> {
    imp::check_private(path)
}

#[cfg(unix)]
mod imp {
    use super::*;
    use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};

    pub fn create_private_dir(dir: &Path) -> io::Result<()> {
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)?;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
    }

    pub fn open_private(path: &Path, opts: &mut OpenOptions) -> io::Result<File> {
        opts.mode(0o600).open(path)
    }

    pub fn check_private(path: &Path) -> io::Result<()> {
        let meta = std::fs::symlink_metadata(path)?;
        // SAFETY: geteuid has no preconditions and cannot fail.
        let euid = unsafe { libc::geteuid() };
        if meta.uid() != euid {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!("{} is not owned by the current user", path.display()),
            ));
        }
        if meta.mode() & 0o077 != 0 {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!(
                    "{} is accessible by other users (mode {:o})",
                    path.display(),
                    meta.mode() & 0o777
                ),
            ));
        }
        Ok(())
    }
}

#[cfg(windows)]
mod imp {
    use super::*;
    use crate::win_security as sec;

    pub fn create_private_dir(dir: &Path) -> io::Result<()> {
        std::fs::create_dir_all(dir)?;
        sec::set_private_dacl(dir, true)
    }

    pub fn open_private(path: &Path, opts: &mut OpenOptions) -> io::Result<File> {
        let file = opts.open(path)?;
        sec::set_private_dacl(path, false)?;
        Ok(file)
    }

    pub fn check_private(path: &Path) -> io::Result<()> {
        sec::check_private(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_new_private_dir_should_pass_check() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("a").join("b");
        create_private_dir(&dir).unwrap();
        check_private(&dir).unwrap();
    }

    #[test]
    fn given_private_file_write_should_round_trip_and_pass_check() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("secret.json");
        write_private_file(&path, b"one").unwrap();
        write_private_file(&path, b"two").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"two");
        check_private(&path).unwrap();
    }

    #[test]
    fn given_existing_file_create_new_should_fail() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("x");
        create_new_private_file(&path).unwrap();
        assert!(create_new_private_file(&path).is_err());
        open_private_file(&path).unwrap();
        check_private(&path).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn given_group_readable_file_should_fail_check() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("x");
        std::fs::write(&path, b"x").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o640)).unwrap();
        assert!(check_private(&path).is_err());
    }

    #[cfg(windows)]
    #[test]
    fn given_everyone_ace_should_fail_check() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("x");
        write_private_file(&path, b"x").unwrap();
        check_private(&path).unwrap();
        crate::win_security::grant_for_test(&path, "WD").unwrap(); // Everyone
        assert!(check_private(&path).is_err());
    }

    #[cfg(windows)]
    #[test]
    fn given_users_ace_should_fail_check() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("x");
        write_private_file(&path, b"x").unwrap();
        crate::win_security::grant_for_test(&path, "BU").unwrap(); // BUILTIN\Users
        assert!(check_private(&path).is_err());
    }

    #[cfg(windows)]
    #[test]
    fn given_default_temp_file_should_fail_check() {
        // A file created without private_fs inherits the temp dir's ACL,
        // which (in a user's TEMP) is not guaranteed private: assert that a
        // file we explicitly open up is rejected, and that re-privatizing fixes it.
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("x");
        std::fs::write(&path, b"x").unwrap();
        crate::win_security::grant_for_test(&path, "WD").unwrap();
        assert!(check_private(&path).is_err());
        crate::win_security::set_private_dacl(&path, false).unwrap();
        check_private(&path).unwrap();
    }
}
