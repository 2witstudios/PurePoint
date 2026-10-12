//! Filesystem boundary against other OS users, not same-account code or root.
//!
//! Directory traversal and metadata operations use held directory descriptors.
//! Unix bind/connect lack portable *at variants: those use the validated path in
//! a trusted ancestor chain. The private directory and lifetime flock exclude
//! other users and cooperating daemons from check/unlink/bind sequences. Unix
//! has no conditional unlink-by-inode; identity rechecks are defensive, not an
//! atomicity guarantee against a malicious process with our own credentials.

// libc mode/device/link field widths differ between Darwin and Linux.
#![allow(clippy::unnecessary_cast)]

use std::collections::VecDeque;
use std::ffi::{OsStr, OsString};
use std::fs::{File, Metadata};
use std::io;
use std::os::unix::fs::MetadataExt;
use std::path::{Component, Path, PathBuf};

use nix::errno::Errno;
use nix::fcntl::{AtFlags, OFlag, openat, readlinkat};
use nix::sys::stat::{FchmodatFlags, Mode, fchmodat, fstatat, mkdirat};
use nix::unistd::{UnlinkatFlags, unlinkat};

pub(super) fn denied(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::PermissionDenied, message)
}

pub(super) fn current_uid() -> u32 {
    // SAFETY: geteuid has no preconditions or memory arguments.
    unsafe { libc::geteuid() }
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Entry {
    dev: u64,
    ino: u64,
    mode: u32,
    uid: u32,
    links: u64,
}

impl Entry {
    pub(super) fn from_file(file: &File) -> io::Result<Self> {
        Ok(Self::from_metadata(&file.metadata()?))
    }

    fn from_metadata(m: &Metadata) -> Self {
        Self {
            dev: m.dev(),
            ino: m.ino(),
            mode: m.mode(),
            uid: m.uid(),
            links: m.nlink(),
        }
    }

    pub(super) fn same_file(self, other: Self) -> bool {
        self.dev == other.dev && self.ino == other.ino && self.kind() == other.kind()
    }

    fn kind(self) -> u32 {
        self.mode & libc::S_IFMT as u32
    }

    pub(super) fn owned_socket(self) -> io::Result<()> {
        if self.kind() != libc::S_IFSOCK as u32 || self.uid != current_uid() || self.links != 1 {
            return Err(denied("socket path must be an owned socket with one link"));
        }
        Ok(())
    }

    pub(super) fn private_file(self) -> io::Result<()> {
        if self.kind() != libc::S_IFREG as u32
            || self.uid != current_uid()
            || self.mode & 0o7777 != 0o600
            || self.links != 1
        {
            return Err(denied(
                "lifecycle file must be an owned regular file with mode 0600 and one link",
            ));
        }
        Ok(())
    }

    fn directory(self, private: bool) -> io::Result<()> {
        if self.kind() != libc::S_IFDIR as u32 {
            return Err(denied("state path must be a directory"));
        }
        if private {
            if self.uid != current_uid() || self.mode & 0o7777 != 0o700 {
                return Err(denied(
                    "state/socket directory must be owned by the daemon user with mode 0700; refusing to change existing permissions",
                ));
            }
        } else if (self.uid != 0 && self.uid != current_uid())
            || (self.mode & 0o022 != 0 && !(self.uid == 0 && self.mode & 0o1000 != 0))
        {
            return Err(denied(
                "state directory ancestor is owned or writable by another user",
            ));
        }
        Ok(())
    }
}

pub(super) struct PrivateDir {
    file: File,
    path: PathBuf,
}

impl PrivateDir {
    pub(super) fn open(path: &Path, create: bool) -> io::Result<Self> {
        let absolute = if path.is_absolute() {
            path.to_owned()
        } else {
            std::env::current_dir()?.join(path)
        };
        let mut components = path_components(&absolute)?;
        let mut file = File::open("/")?;
        let mut resolved = PathBuf::from("/");
        let mut aliases = 0;
        while let Some(name) = components.pop_front() {
            Entry::from_file(&file)?.directory(false)?;
            reject_acl_grants(&file)?;
            let entry = match entry_at(&file, &name)? {
                Some(entry) => entry,
                None if create => {
                    match mkdirat(&file, name.as_os_str(), Mode::S_IRWXU) {
                        Ok(()) => file.sync_all()?,
                        Err(Errno::EEXIST) => {}
                        Err(e) => return Err(e.into()),
                    }
                    entry_at(&file, &name)?
                        .ok_or_else(|| denied("state directory disappeared during creation"))?
                }
                None => return Err(io::Error::from(io::ErrorKind::NotFound)),
            };
            if entry.kind() == libc::S_IFLNK as u32 {
                // macOS /var and /tmp are root-owned aliases. Only permit
                // root-controlled ancestor aliases, never the private leaf.
                if components.is_empty()
                    || entry.uid != 0
                    || Entry::from_file(&file)?.uid != 0
                    || aliases == 40
                {
                    return Err(denied("symlink in private state directory path"));
                }
                aliases += 1;
                let target = PathBuf::from(readlinkat(&file, name.as_os_str())?);
                let mut expanded = path_components(&target)?;
                expanded.append(&mut components);
                components = expanded;
                if target.is_absolute() {
                    file = File::open("/")?;
                    resolved = PathBuf::from("/");
                }
                continue;
            }
            let child = File::from(openat(
                &file,
                name.as_os_str(),
                OFlag::O_RDONLY | OFlag::O_DIRECTORY | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC,
                Mode::empty(),
            )?);
            let opened = Entry::from_file(&child)?;
            if !entry.same_file(opened) {
                return Err(denied("state directory changed during open"));
            }
            opened.directory(components.is_empty())?;
            reject_acl_grants(&child)?;
            file = child;
            resolved.push(name);
        }
        Entry::from_file(&file)?.directory(true)?;
        reject_acl_grants(&file)?;
        Ok(Self {
            file,
            path: resolved,
        })
    }

    pub(super) fn path_for(&self, name: &OsStr) -> PathBuf {
        self.path.join(name)
    }

    pub(super) fn verify(&self) -> io::Result<()> {
        let held = Entry::from_file(&self.file)?;
        held.directory(true)?;
        let current = Self::open(&self.path, false)?;
        if !held.same_file(Entry::from_file(&current.file)?) {
            return Err(denied("private state directory was replaced"));
        }
        Ok(())
    }

    pub(super) fn entry(&self, name: &OsStr) -> io::Result<Option<Entry>> {
        entry_at(&self.file, name)
    }

    pub(super) fn require_identity(&self, name: &OsStr, expected: Entry) -> io::Result<Entry> {
        let entry = self
            .entry(name)?
            .ok_or_else(|| denied("lifecycle entry disappeared"))?;
        if !entry.same_file(expected) {
            return Err(denied("lifecycle entry was replaced"));
        }
        Ok(entry)
    }

    pub(super) fn open_file(&self, name: &OsStr, create: bool) -> io::Result<File> {
        self.open_file_attempt(name, create, true)
    }

    fn open_file_attempt(&self, name: &OsStr, create: bool, retry: bool) -> io::Result<File> {
        self.verify()?;
        let before = self.entry(name)?;
        if let Some(entry) = before {
            entry.private_file()?;
        }
        let mut flags = OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC | OFlag::O_NONBLOCK;
        flags |= if create {
            OFlag::O_RDWR
        } else {
            OFlag::O_RDONLY
        };
        if create && before.is_none() {
            flags |= OFlag::O_CREAT | OFlag::O_EXCL;
        }
        let fd = match openat(&self.file, name, flags, Mode::S_IRUSR | Mode::S_IWUSR) {
            // A competing startup may have just created the shared lock inode.
            // Revalidate it once rather than truncating or replacing anything.
            Err(Errno::EEXIST) if before.is_none() && create && retry => {
                return self.open_file_attempt(name, create, false);
            }
            result => result?,
        };
        let file = File::from(fd);
        let opened = Entry::from_file(&file)?;
        opened.private_file()?;
        reject_acl_grants(&file)?;
        if before.is_some_and(|entry| !entry.same_file(opened)) {
            return Err(denied("lifecycle file changed during open"));
        }
        self.require_identity(name, opened)?.private_file()?;
        Ok(file)
    }

    pub(super) fn protect_socket(&self, name: &OsStr, expected: Entry) -> io::Result<()> {
        self.verify()?;
        self.require_identity(name, expected)?.owned_socket()?;
        // The directory already excludes other users while bind/chmod run.
        // No process-global umask change (the daemon has concurrent threads).
        fchmodat(
            &self.file,
            name,
            Mode::S_IRUSR | Mode::S_IWUSR,
            FchmodatFlags::NoFollowSymlink,
        )?;
        let entry = self.require_identity(name, expected)?;
        entry.owned_socket()?;
        if entry.mode & 0o7777 != 0o600 {
            return Err(denied("socket is not private"));
        }
        self.sync()
    }

    pub(super) fn remove_socket(&self, name: &OsStr, expected: Entry) -> io::Result<()> {
        self.verify()?;
        self.require_identity(name, expected)?.owned_socket()?;
        unlinkat(&self.file, name, UnlinkatFlags::NoRemoveDir)?;
        self.sync()
    }

    pub(super) fn remove_file(&self, name: &OsStr, expected: &File) -> io::Result<()> {
        self.verify()?;
        self.require_identity(name, Entry::from_file(expected)?)?
            .private_file()?;
        unlinkat(&self.file, name, UnlinkatFlags::NoRemoveDir)?;
        self.sync()
    }

    pub(super) fn sync(&self) -> io::Result<()> {
        self.file.sync_all()
    }
}

fn path_components(path: &Path) -> io::Result<VecDeque<OsString>> {
    path.components()
        .filter_map(|c| match c {
            Component::Normal(name) => Some(Ok(name.to_owned())),
            Component::RootDir | Component::CurDir => None,
            _ => Some(Err(denied(
                "parent traversal is not allowed in state paths",
            ))),
        })
        .collect()
}

fn entry_at(dir: &File, name: &OsStr) -> io::Result<Option<Entry>> {
    match fstatat(dir, name, AtFlags::AT_SYMLINK_NOFOLLOW) {
        Ok(s) => Ok(Some(Entry {
            dev: s.st_dev as u64,
            ino: s.st_ino,
            mode: s.st_mode as u32,
            uid: s.st_uid,
            links: s.st_nlink as u64,
        })),
        Err(Errno::ENOENT) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

// Darwin ACL grants can bypass mode bits. Conservatively reject grant entries
// (including inherited grants); deny-only ACLs such as home-directory deletion
// protection remain valid. Linux POSIX ACL grants are constrained by the mode
// group mask, already checked above for files/directories and ancestors.
#[cfg(target_os = "macos")]
fn reject_acl_grants(file: &File) -> io::Result<()> {
    use std::ffi::c_void;
    use std::os::fd::AsRawFd;
    unsafe extern "C" {
        fn acl_get_fd_np(fd: libc::c_int, kind: libc::c_int) -> *mut c_void;
        fn acl_get_entry(acl: *mut c_void, id: libc::c_int, entry: *mut *mut c_void)
        -> libc::c_int;
        fn acl_get_tag_type(entry: *mut c_void, tag: *mut libc::c_int) -> libc::c_int;
        fn acl_free(acl: *mut c_void) -> libc::c_int;
    }
    struct Acl(*mut c_void);
    impl Drop for Acl {
        fn drop(&mut self) {
            // SAFETY: this owns a non-null ACL allocated by acl_get_fd_np.
            unsafe {
                acl_free(self.0);
            }
        }
    }
    const ACL_TYPE_EXTENDED: libc::c_int = 0x100;
    const ACL_EXTENDED_DENY: libc::c_int = 2;
    // SAFETY: file holds a live descriptor; the ACL is independently allocated.
    let acl = unsafe { acl_get_fd_np(file.as_raw_fd(), ACL_TYPE_EXTENDED) };
    if acl.is_null() {
        let error = io::Error::last_os_error();
        // A live descriptor with no extended ACL reports ENOENT on Darwin.
        return if matches!(error.raw_os_error(), Some(libc::ENOTSUP | libc::ENOENT)) {
            Ok(())
        } else {
            Err(error)
        };
    }
    let acl = Acl(acl);
    let mut id = 0; // ACL_FIRST_ENTRY; ACL_NEXT_ENTRY is -1 on Darwin.
    loop {
        let mut entry = std::ptr::null_mut();
        // SAFETY: acl is live, and entry points to writable pointer storage.
        if unsafe { acl_get_entry(acl.0, id, &mut entry) } != 0 {
            let error = io::Error::last_os_error();
            // Darwin reports exhaustion as EINVAL, including an empty ACL.
            return if error.raw_os_error() == Some(libc::EINVAL) {
                Ok(())
            } else {
                Err(error)
            };
        }
        let mut tag = 0;
        // SAFETY: entry was returned by the live ACL, tag is writable storage.
        if unsafe { acl_get_tag_type(entry, &mut tag) } != 0 {
            return Err(io::Error::last_os_error());
        }
        if tag != ACL_EXTENDED_DENY {
            return Err(denied(
                "ACL grants are not allowed on private state files or directory ancestors",
            ));
        }
        id = -1;
    }
}

#[cfg(not(target_os = "macos"))]
fn reject_acl_grants(_file: &File) -> io::Result<()> {
    Ok(())
}
