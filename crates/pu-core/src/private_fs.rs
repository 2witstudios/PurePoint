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
    imp::open_private(path, true)
}

/// Open (creating if missing, never truncating) a private file for writing,
/// e.g. a lock file.
pub fn open_private_file(path: &Path) -> io::Result<File> {
    imp::open_private(path, false)
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
        crate::fs_retry::rename(&temp, path)?;
        sync_dir(parent)
    };
    write().inspect_err(|_| {
        let _ = crate::fs_retry::remove_file(&temp);
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

/// Create the single directory `dir` privately, failing if it exists (its
/// parent must exist). Unix: `mkdir(dir, 0700)`.
pub fn create_new_private_dir(dir: &Path) -> io::Result<()> {
    imp::create_new_private_dir(dir)
}

/// Open `path` read-only without following a final symlink or reparse point
/// and without blocking on a FIFO.
///
/// Unix: `O_NOFOLLOW | O_NONBLOCK`. Windows: `FILE_FLAG_OPEN_REPARSE_POINT`
/// opens a symlink or junction itself, which is then refused by its
/// `FILE_ATTRIBUTE_REPARSE_POINT`; anything but a disk file (a device or
/// pipe path) is refused too. The file is shared for read, write and
/// delete, so writers and renames are never blocked by this handle.
pub fn open_no_follow(path: &Path) -> io::Result<File> {
    imp::open_no_follow(path)
}

/// [`open_no_follow`], then require a regular file owned by the current user.
pub fn open_private_no_follow(path: &Path) -> io::Result<File> {
    let file = open_no_follow(path)?;
    if !file.metadata()?.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{} is not a regular file", path.display()),
        ));
    }
    if !owned_by_current_user(&file)? {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("{} is not owned by the current user", path.display()),
        ));
    }
    Ok(file)
}

/// The open file's owner is the current user. Unix: `st_uid == geteuid()`.
/// Windows: the owner SID is the user (or SYSTEM/Administrators, as in
/// [`check_private`]).
pub fn owned_by_current_user(file: &File) -> io::Result<bool> {
    imp::owned_by_current_user(file)
}

/// [`check_private`] on an open file, without a second path lookup.
pub fn check_private_file(file: &File, label: &Path) -> io::Result<()> {
    imp::check_private_file(file, label)
}

/// A stable identity for an open file: `(st_dev, st_ino)` on Unix, the
/// volume serial number and file index on Windows.
pub fn file_identity(file: &File) -> io::Result<(u64, u64)> {
    imp::file_identity(file)
}

/// Move the finished temp file `from` over `to`, keeping `to`'s ACL and
/// attributes on Windows (`ReplaceFileW`; a plain rename when `to` does not
/// exist). Unix: `rename(2)`; the caller already gave `from` the old mode.
/// `from` must be closed.
pub fn replace_file(from: &Path, to: &Path) -> io::Result<()> {
    imp::replace_file(from, to)
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

    pub fn open_private(path: &Path, create_new: bool) -> io::Result<File> {
        let mut opts = OpenOptions::new();
        if create_new {
            opts.write(true).create_new(true);
        } else {
            opts.write(true).create(true).truncate(false);
        }
        opts.mode(0o600).open(path)
    }

    pub fn create_new_private_dir(dir: &Path) -> io::Result<()> {
        std::fs::DirBuilder::new().mode(0o700).create(dir)
    }

    pub fn open_no_follow(path: &Path) -> io::Result<File> {
        OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(path)
    }

    pub fn owned_by_current_user(file: &File) -> io::Result<bool> {
        // SAFETY: geteuid has no preconditions and cannot fail.
        Ok(file.metadata()?.uid() == unsafe { libc::geteuid() })
    }

    pub fn check_private_file(file: &File, label: &Path) -> io::Result<()> {
        check_meta(&file.metadata()?, label)
    }

    pub fn file_identity(file: &File) -> io::Result<(u64, u64)> {
        let meta = file.metadata()?;
        Ok((meta.dev(), meta.ino()))
    }

    pub fn replace_file(from: &Path, to: &Path) -> io::Result<()> {
        crate::fs_retry::rename(from, to)
    }

    pub fn check_private(path: &Path) -> io::Result<()> {
        check_meta(&std::fs::symlink_metadata(path)?, path)
    }

    fn check_meta(meta: &std::fs::Metadata, path: &Path) -> io::Result<()> {
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
    use std::os::windows::fs::OpenOptionsExt;
    use std::os::windows::io::{AsRawHandle, FromRawHandle};
    use windows_sys::Win32::Foundation::{GENERIC_WRITE, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::Security::SECURITY_ATTRIBUTES;
    use windows_sys::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, CREATE_NEW, CreateDirectoryW, CreateFileW,
        FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_NORMAL, FILE_ATTRIBUTE_REPARSE_POINT,
        FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_DELETE,
        FILE_SHARE_READ, FILE_SHARE_WRITE, FILE_TYPE_DISK, GetFileInformationByHandle, GetFileType,
        OPEN_ALWAYS, OPEN_EXISTING, REPLACEFILE_IGNORE_MERGE_ERRORS, ReplaceFileW,
    };

    const READ_CONTROL: u32 = 0x0002_0000;
    const WRITE_DAC: u32 = 0x0004_0000;
    const ERROR_ALREADY_EXISTS: i32 = 183;

    fn reparse_refused(path: &Path) -> io::Error {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "{} is a symbolic link or junction, which is never followed",
                path.display()
            ),
        )
    }

    /// `CreateFileW` on `path` itself (never following a final reparse
    /// point), shared like std opens. Returns the file and whether it
    /// already existed (`OPEN_ALWAYS`).
    fn create_file(
        path: &Path,
        access: u32,
        disposition: u32,
        flags: u32,
        sa: Option<&SECURITY_ATTRIBUTES>,
    ) -> io::Result<(File, bool)> {
        let name = sec::wide(path.as_os_str());
        // SAFETY: name is NUL-terminated; sa (if any) and its descriptor
        // outlive the call.
        let handle = unsafe {
            CreateFileW(
                name.as_ptr(),
                access,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                sa.map_or(std::ptr::null(), |sa| sa as *const _),
                disposition,
                flags | FILE_FLAG_OPEN_REPARSE_POINT,
                std::ptr::null_mut(),
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }
        let existed = io::Error::last_os_error().raw_os_error() == Some(ERROR_ALREADY_EXISTS);
        // SAFETY: CreateFileW returned a fresh handle we now own.
        Ok((unsafe { File::from_raw_handle(handle as _) }, existed))
    }

    fn security_attributes(sd: &sec::SecurityDescriptor) -> SECURITY_ATTRIBUTES {
        SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: sd.as_ptr(),
            bInheritHandle: 0,
        }
    }

    pub fn create_private_dir(dir: &Path) -> io::Result<()> {
        if let Some(parent) = dir.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent)?;
        }
        match create_new_private_dir(dir) {
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
            other => return other,
        }
        // An existing directory: fix its DACL through a handle to it, never
        // to whatever a junction there points at.
        let (handle, _) = create_file(
            dir,
            READ_CONTROL | WRITE_DAC,
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS,
            None,
        )?;
        let attrs = by_handle(&handle)?.dwFileAttributes;
        if attrs & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(reparse_refused(dir));
        }
        if attrs & FILE_ATTRIBUTE_DIRECTORY == 0 {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!("{} exists and is not a directory", dir.display()),
            ));
        }
        sec::set_private_dacl_handle(&handle, true)
    }

    /// A new file is created with the private DACL; an existing one (open
    /// mode only) has its DACL replaced through the handle.
    pub fn open_private(path: &Path, create_new: bool) -> io::Result<File> {
        let sd = sec::private_descriptor(false)?;
        let sa = security_attributes(&sd);
        let (file, existed) = create_file(
            path,
            GENERIC_WRITE | READ_CONTROL | WRITE_DAC,
            if create_new { CREATE_NEW } else { OPEN_ALWAYS },
            FILE_ATTRIBUTE_NORMAL,
            Some(&sa),
        )?;
        if existed {
            if by_handle(&file)?.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
                return Err(reparse_refused(path));
            }
            sec::set_private_dacl_handle(&file, false)?;
        }
        Ok(file)
    }

    pub fn check_private(path: &Path) -> io::Result<()> {
        sec::check_private(path)
    }

    pub fn create_new_private_dir(dir: &Path) -> io::Result<()> {
        let sd = sec::private_descriptor(true)?;
        let sa = security_attributes(&sd);
        let name = sec::wide(dir.as_os_str());
        // SAFETY: name is NUL-terminated; sa and its descriptor outlive the call.
        if unsafe { CreateDirectoryW(name.as_ptr(), &sa) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    fn raw(file: &File) -> windows_sys::Win32::Foundation::HANDLE {
        file.as_raw_handle() as _
    }

    fn by_handle(file: &File) -> io::Result<BY_HANDLE_FILE_INFORMATION> {
        // SAFETY: zeroed is a valid BY_HANDLE_FILE_INFORMATION; the handle is live.
        let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
        if unsafe { GetFileInformationByHandle(raw(file), &mut info) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(info)
    }

    pub fn open_no_follow(path: &Path) -> io::Result<File> {
        let file = OpenOptions::new()
            .read(true)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
            .open(path)?;
        // SAFETY: the handle is live.
        if unsafe { GetFileType(raw(&file)) } != FILE_TYPE_DISK {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("{} is not a file", path.display()),
            ));
        }
        if by_handle(&file)?.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "{} is a symbolic link or junction, which is never followed",
                    path.display()
                ),
            ));
        }
        Ok(file)
    }

    pub fn owned_by_current_user(file: &File) -> io::Result<bool> {
        sec::handle_owned_by_current_user(raw(file))
    }

    pub fn check_private_file(file: &File, label: &Path) -> io::Result<()> {
        sec::check_private_handle(raw(file), label)
    }

    pub fn file_identity(file: &File) -> io::Result<(u64, u64)> {
        let info = by_handle(file)?;
        Ok((
            u64::from(info.dwVolumeSerialNumber),
            (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow),
        ))
    }

    pub fn replace_file(from: &Path, to: &Path) -> io::Result<()> {
        const ERROR_FILE_NOT_FOUND: i32 = 2;
        const ERROR_SHARING_VIOLATION: i32 = 32;
        const ERROR_UNABLE_TO_REMOVE_REPLACED: i32 = 1175;
        let target = sec::wide(to.as_os_str());
        let source = sec::wide(from.as_os_str());
        let replace = || {
            // SAFETY: both names are NUL-terminated; the reserved arguments are null.
            let ok = unsafe {
                ReplaceFileW(
                    target.as_ptr(),
                    source.as_ptr(),
                    std::ptr::null(),
                    REPLACEFILE_IGNORE_MERGE_ERRORS,
                    std::ptr::null(),
                    std::ptr::null(),
                )
            };
            if ok != 0 {
                return Ok(());
            }
            let err = io::Error::last_os_error();
            // The replaced file is held open: a lock, retried like one.
            if err.raw_os_error() == Some(ERROR_UNABLE_TO_REMOVE_REPLACED) {
                return Err(io::Error::from_raw_os_error(ERROR_SHARING_VIOLATION));
            }
            Err(err)
        };
        match crate::fs_retry::retry_op(to, replace) {
            Err(err) if err.raw_os_error() == Some(ERROR_FILE_NOT_FOUND) && !to.exists() => {
                // No file to replace: a plain move.
                crate::fs_retry::rename(from, to)
            }
            result => result,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Give `dir` an inheritable Everyone entry, as a careless parent might.
    #[cfg(windows)]
    fn leaky_parent(dir: &Path) {
        let user = crate::win_security::current_user_sid()
            .unwrap()
            .to_string_sid()
            .unwrap();
        let sddl = format!(
            "{}(A;OICI;FA;;;WD)",
            crate::win_security::private_sddl(&user, true)
        );
        crate::win_security::apply_sddl(dir, &sddl).unwrap();
        assert!(crate::win_security::check_private(dir).is_err());
    }

    #[cfg(windows)]
    #[test]
    fn given_leaky_parent_should_create_private_paths_without_inheriting() {
        let tmp = tempfile::tempdir().unwrap();
        leaky_parent(tmp.path());
        let new_dir = tmp.path().join("new");
        create_new_private_dir(&new_dir).unwrap();
        check_private(&new_dir).unwrap();
        let nested = tmp.path().join("a").join("b");
        create_private_dir(&nested).unwrap();
        check_private(&nested).unwrap();
        let file = tmp.path().join("f");
        drop(create_new_private_file(&file).unwrap());
        check_private(&file).unwrap();
        // Existing paths with an inherited DACL are fixed by handle.
        let old_dir = tmp.path().join("old");
        std::fs::create_dir(&old_dir).unwrap();
        assert!(check_private(&old_dir).is_err());
        create_private_dir(&old_dir).unwrap();
        check_private(&old_dir).unwrap();
        let old_file = tmp.path().join("old.lock");
        std::fs::write(&old_file, b"x").unwrap();
        assert!(check_private(&old_file).is_err());
        drop(open_private_file(&old_file).unwrap());
        check_private(&old_file).unwrap();
        assert_eq!(std::fs::read(&old_file).unwrap(), b"x");
    }

    #[cfg(windows)]
    #[test]
    fn given_junction_should_refuse_to_make_it_private_without_touching_target() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("target");
        std::fs::create_dir(&target).unwrap();
        leaky_parent(&target);
        let link = tmp.path().join("link");
        let status = std::process::Command::new("cmd")
            .args(["/d", "/c", "mklink", "/J"])
            .arg(&link)
            .arg(&target)
            .stdout(std::process::Stdio::null())
            .status()
            .unwrap();
        assert!(status.success());
        let err = create_private_dir(&link).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput, "{err}");
        assert!(check_private(&target).is_err(), "target DACL was changed");
    }

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

    #[test]
    fn given_owned_regular_file_should_open_without_following() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("x");
        write_private_file(&path, b"x").unwrap();
        let file = open_private_no_follow(&path).unwrap();
        assert!(owned_by_current_user(&file).unwrap());
        check_private_file(&file, &path).unwrap();
        let again = open_no_follow(&path).unwrap();
        assert_eq!(
            file_identity(&file).unwrap(),
            file_identity(&again).unwrap()
        );
        let other = tmp.path().join("y");
        write_private_file(&other, b"y").unwrap();
        assert_ne!(
            file_identity(&file).unwrap(),
            file_identity(&open_no_follow(&other).unwrap()).unwrap()
        );
    }

    #[test]
    fn given_new_private_dir_should_refuse_existing() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("d");
        create_new_private_dir(&dir).unwrap();
        check_private(&dir).unwrap();
        assert!(create_new_private_dir(&dir).is_err());
    }

    #[test]
    fn given_replace_should_swap_contents_and_create_missing_target() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("t");
        let a = tmp.path().join("a");
        std::fs::write(&a, b"first").unwrap();
        replace_file(&a, &target).unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"first");
        let b = tmp.path().join("b");
        std::fs::write(&b, b"second").unwrap();
        replace_file(&b, &target).unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"second");
        assert!(!a.exists() && !b.exists());
    }

    #[cfg(unix)]
    #[test]
    fn given_symlink_should_refuse_to_follow() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("target");
        std::fs::write(&target, b"x").unwrap();
        let link = tmp.path().join("link");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        assert!(open_no_follow(&link).is_err());
    }

    #[cfg(windows)]
    #[test]
    fn given_replace_should_keep_the_replaced_files_acl() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("t");
        write_private_file(&target, b"old").unwrap();
        crate::win_security::grant_for_test(&target, "WD").unwrap();
        let temp = tmp.path().join("new");
        write_private_file(&temp, b"new").unwrap();
        check_private(&temp).unwrap();
        replace_file(&temp, &target).unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"new");
        // The Everyone entry came from the replaced file, not the temp file.
        assert!(check_private(&target).is_err());
    }

    /// Symlinks need a privilege or Developer Mode; junctions never do.
    #[cfg(windows)]
    #[test]
    fn given_symlink_or_junction_should_refuse_to_follow() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("target");
        write_private_file(&target, b"x").unwrap();
        let link = tmp.path().join("link");
        if std::os::windows::fs::symlink_file(&target, &link).is_ok() {
            let err = open_no_follow(&link).unwrap_err();
            assert!(err.to_string().contains("never followed"), "{err}");
            assert!(open_private_no_follow(&link).is_err());
        }
        let dir = tmp.path().join("dir");
        std::fs::create_dir(&dir).unwrap();
        std::fs::write(dir.join("inner"), b"x").unwrap();
        let junction = tmp.path().join("junction");
        let status = std::process::Command::new("cmd")
            .args(["/d", "/c", "mklink", "/J"])
            .arg(&junction)
            .arg(&dir)
            .stdout(std::process::Stdio::null())
            .status()
            .unwrap();
        assert!(status.success());
        assert!(open_no_follow(&junction).is_err());
        // Opening through the junction still works for ordinary opens; only
        // the final component is never followed.
        assert!(open_no_follow(&junction.join("inner")).is_ok());
        assert!(open_no_follow(Path::new(r"\\.\NUL")).is_err());
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
