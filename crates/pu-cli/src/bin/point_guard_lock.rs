//! Runtime-only holder of a lock shared with the owning Node file description.
//! Never unlock explicitly: the parent's retained descriptor protects pending IO.
#[cfg(unix)]
use clap::Parser;
#[cfg(unix)]
use serde_json::json;
#[cfg(unix)]
use std::fs::{File, OpenOptions};
#[cfg(unix)]
use std::io::{Read, Write};
#[cfg(unix)]
use std::os::fd::{AsRawFd, FromRawFd};
#[cfg(unix)]
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
#[cfg(unix)]
use std::path::PathBuf;

#[cfg(unix)]
#[derive(Parser)]
#[command(hide = true)]
struct Args {
    #[arg(long)]
    file: PathBuf,
    #[arg(long)]
    nonce: String,
    #[arg(long)]
    owner_pid: u32,
}
#[cfg(unix)]
fn private(metadata: &std::fs::Metadata, directory: bool) -> bool {
    metadata.uid() == unsafe { libc::geteuid() }
        && metadata.mode() & 0o777 == if directory { 0o700 } else { 0o600 }
        && if directory {
            metadata.is_dir()
        } else {
            metadata.is_file() && metadata.nlink() == 1
        }
}
#[cfg(unix)]
fn validate(file: &File, args: &Args) -> Result<(), &'static str> {
    let metadata = file.metadata().map_err(|_| "lock_private")?;
    if !private(&metadata, false) {
        return Err("lock_private");
    }
    let parent = args.file.parent().ok_or("lock_private")?;
    let directory = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(parent)
        .map_err(|_| "lock_private")?;
    if !private(&directory.metadata().map_err(|_| "lock_private")?, true) {
        return Err("lock_private");
    }
    let path_metadata = std::fs::symlink_metadata(&args.file).map_err(|_| "lock_path_changed")?;
    if !private(&path_metadata, false) {
        return Err("lock_private");
    }
    if path_metadata.dev() != metadata.dev() || path_metadata.ino() != metadata.ino() {
        return Err("lock_path_changed");
    }
    Ok(())
}
#[cfg(unix)]
fn hold(args: &Args) -> Result<(), &'static str> {
    if !args.file.is_absolute()
        || args.nonce.is_empty()
        || args.nonce.len() > 128
        || unsafe { libc::getppid() } as u32 != args.owner_pid
    {
        return Err("lock_helper_failed");
    }
    // fd3 is explicitly inherited from Node and shares its open-file description.
    // Reject missing descriptors before creating an owning Rust File.
    if unsafe { libc::fcntl(3, libc::F_GETFD) } < 0 {
        return Err("lock_helper_failed");
    }
    let file = unsafe { File::from_raw_fd(3) };
    if unsafe { libc::fcntl(3, libc::F_SETFD, libc::FD_CLOEXEC) } < 0 {
        return Err("lock_helper_failed");
    }
    validate(&file, args)?;
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } < 0 {
        return Err(
            if std::io::Error::last_os_error().kind() == std::io::ErrorKind::WouldBlock {
                "lock_busy"
            } else {
                "lock_helper_failed"
            },
        );
    }
    validate(&file, args)?;
    let metadata = file.metadata().map_err(|_| "lock_helper_failed")?;
    let ready = json!({"schemaVersion":1,"type":"ready","nonce":args.nonce,"pid":std::process::id(),"device":metadata.dev().to_string(),"inode":metadata.ino().to_string()});
    let mut stdout = std::io::stdout().lock();
    writeln!(stdout, "{ready}")
        .and_then(|_| stdout.flush())
        .map_err(|_| "lock_helper_failed")?;
    drop(stdout);
    let mut buffer = [0u8; 256];
    loop {
        match std::io::stdin().read(&mut buffer) {
            Ok(0) => break,
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(_) => return Err("lock_helper_failed"),
        }
    }
    // Drop only. LOCK_UN would unlock the parent's shared OFD during pending IO.
    drop(file);
    Ok(())
}
#[cfg(windows)]
fn main() {
    windows::main();
}

/// Windows: the parent opens the lock file and hands the helper that handle
/// by inheritance, either listed with `PROC_THREAD_ATTRIBUTE_HANDLE_LIST` and
/// named with `--handle`, or (Node/libuv) as the CRT descriptor 3 slot of
/// `STARTUPINFO.lpReserved2`. The helper then takes `LockFileEx` on it.
///
/// Windows releases a process's byte-range locks when that process closes
/// its handle, so the lock alone ends with the helper. The bridge also opens
/// the file without sharing (`UV_FS_O_EXLOCK`), which keeps a second owner
/// out until the parent closes its handle, as the shared OFD does on Unix.
#[cfg(windows)]
mod windows {
    use clap::Parser;
    use serde_json::json;
    use std::fs::{File, OpenOptions};
    use std::io::{Read, Write};
    use std::os::windows::fs::OpenOptionsExt;
    use std::os::windows::io::{AsRawHandle, FromRawHandle};
    use std::path::{Path, PathBuf};
    use windows_sys::Win32::Foundation::{
        CloseHandle, ERROR_LOCK_VIOLATION, GetHandleInformation, HANDLE, HANDLE_FLAG_INHERIT,
        INVALID_HANDLE_VALUE, SetHandleInformation,
    };
    use windows_sys::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT,
        FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_READ_ATTRIBUTES,
        FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, FILE_TYPE_DISK,
        GetFileInformationByHandle, GetFileType, LOCKFILE_EXCLUSIVE_LOCK,
        LOCKFILE_FAIL_IMMEDIATELY, LockFileEx, READ_CONTROL,
    };
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
        TH32CS_SNAPPROCESS,
    };
    use windows_sys::Win32::System::IO::OVERLAPPED;
    use windows_sys::Win32::System::Threading::{GetStartupInfoW, STARTUPINFOW};

    #[derive(Parser)]
    #[command(hide = true)]
    struct Args {
        #[arg(long)]
        file: PathBuf,
        #[arg(long)]
        nonce: String,
        #[arg(long)]
        owner_pid: u32,
        /// Inherited handle value; without it, CRT descriptor 3.
        #[arg(long)]
        handle: Option<u64>,
    }

    fn info(file: &File) -> Result<BY_HANDLE_FILE_INFORMATION, &'static str> {
        // SAFETY: zeroed is a valid BY_HANDLE_FILE_INFORMATION; the handle is live.
        let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
        if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) } == 0 {
            return Err("lock_private");
        }
        Ok(info)
    }

    fn identity(info: &BY_HANDLE_FILE_INFORMATION) -> (u64, u64) {
        (
            u64::from(info.dwVolumeSerialNumber),
            (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow),
        )
    }

    /// A plain, singly linked file or a plain directory: never a reparse
    /// point (symlink, junction), owned by this user with an owner-only DACL.
    fn private(file: &File, label: &Path, directory: bool) -> Result<(), &'static str> {
        let info = info(file)?;
        let is_dir = info.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY != 0;
        if info.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
            || is_dir != directory
            || (!directory && info.nNumberOfLinks != 1)
            // SAFETY: the handle is live.
            || unsafe { GetFileType(file.as_raw_handle()) } != FILE_TYPE_DISK
        {
            return Err("lock_private");
        }
        pu_core::private_fs::check_private_file(file, label).map_err(|_| "lock_private")
    }

    /// Open the entry itself (a reparse point is opened, not followed) for
    /// attributes and security only: no data access, so the parent's
    /// no-sharing open never conflicts with it.
    fn open_entry(path: &Path) -> std::io::Result<File> {
        OpenOptions::new()
            .access_mode(FILE_READ_ATTRIBUTES | READ_CONTROL)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS)
            .open(path)
    }

    fn validate(file: &File, args: &Args) -> Result<(), &'static str> {
        private(file, &args.file, false)?;
        let parent = args.file.parent().ok_or("lock_private")?;
        let directory = open_entry(parent).map_err(|_| "lock_private")?;
        private(&directory, parent, true)?;
        let entry = open_entry(&args.file).map_err(|_| "lock_path_changed")?;
        private(&entry, &args.file, false)?;
        if identity(&info(&entry)?) != identity(&info(file)?) {
            return Err("lock_path_changed");
        }
        Ok(())
    }

    fn parent_pid() -> Option<u32> {
        let me = std::process::id();
        // SAFETY: the snapshot handle is closed below; the entry is sized.
        unsafe {
            let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
            if snapshot == INVALID_HANDLE_VALUE {
                return None;
            }
            let mut entry: PROCESSENTRY32W = std::mem::zeroed();
            entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
            let mut found = None;
            let mut more = Process32FirstW(snapshot, &mut entry) != 0;
            while more {
                if entry.th32ProcessID == me {
                    found = Some(entry.th32ParentProcessID);
                    break;
                }
                more = Process32NextW(snapshot, &mut entry) != 0;
            }
            CloseHandle(snapshot);
            found
        }
    }

    /// The handle libuv passed as CRT descriptor 3: `lpReserved2` holds a
    /// count, one flag byte per descriptor, then one handle per descriptor.
    fn crt_descriptor_3() -> Option<HANDLE> {
        const FD: usize = 3;
        const FOPEN: u8 = 0x01;
        // SAFETY: GetStartupInfoW fills the struct; lpReserved2 is read only
        // within cbReserved2 bytes.
        unsafe {
            let mut startup: STARTUPINFOW = std::mem::zeroed();
            startup.cb = std::mem::size_of::<STARTUPINFOW>() as u32;
            GetStartupInfoW(&mut startup);
            let size = usize::from(startup.cbReserved2);
            let base = startup.lpReserved2;
            if base.is_null() || size < 4 {
                return None;
            }
            let count = usize::try_from(std::ptr::read_unaligned(base as *const i32)).ok()?;
            let handle_size = std::mem::size_of::<HANDLE>();
            if count <= FD || size < 4 + count + count * handle_size {
                return None;
            }
            if *base.add(4 + FD) & FOPEN == 0 {
                return None;
            }
            let handle =
                std::ptr::read_unaligned(base.add(4 + count + FD * handle_size) as *const HANDLE);
            (!handle.is_null() && handle != INVALID_HANDLE_VALUE).then_some(handle)
        }
    }

    fn hold(args: &Args) -> Result<(), &'static str> {
        if !args.file.is_absolute()
            || args.nonce.is_empty()
            || args.nonce.len() > 128
            || parent_pid() != Some(args.owner_pid)
        {
            return Err("lock_helper_failed");
        }
        let handle = match args.handle {
            Some(value) => usize::try_from(value).map_err(|_| "lock_helper_failed")? as HANDLE,
            None => crt_descriptor_3().ok_or("lock_helper_failed")?,
        };
        // Reject a missing or non-disk handle before creating an owning File.
        let mut flags = 0u32;
        // SAFETY: both calls only query the handle table entry.
        if unsafe { GetHandleInformation(handle, &mut flags) } == 0
            || unsafe { GetFileType(handle) } != FILE_TYPE_DISK
        {
            return Err("lock_helper_failed");
        }
        // SAFETY: the handle was inherited for this process and is owned here.
        let file = unsafe { File::from_raw_handle(handle as _) };
        // Never pass it on to anything this process could start.
        // SAFETY: a live handle owned by `file`.
        if unsafe { SetHandleInformation(file.as_raw_handle(), HANDLE_FLAG_INHERIT, 0) } == 0 {
            return Err("lock_helper_failed");
        }
        validate(&file, args)?;
        // SAFETY: a zeroed OVERLAPPED is offset 0; the whole range is locked.
        let mut overlapped: OVERLAPPED = unsafe { std::mem::zeroed() };
        let locked = unsafe {
            LockFileEx(
                file.as_raw_handle(),
                LOCKFILE_EXCLUSIVE_LOCK | LOCKFILE_FAIL_IMMEDIATELY,
                0,
                u32::MAX,
                u32::MAX,
                &mut overlapped,
            )
        };
        if locked == 0 {
            let busy =
                std::io::Error::last_os_error().raw_os_error() == Some(ERROR_LOCK_VIOLATION as i32);
            return Err(if busy {
                "lock_busy"
            } else {
                "lock_helper_failed"
            });
        }
        validate(&file, args)?;
        let (device, inode) = identity(&info(&file).map_err(|_| "lock_helper_failed")?);
        let ready = json!({"schemaVersion":1,"type":"ready","nonce":args.nonce,"pid":std::process::id(),"device":device.to_string(),"inode":inode.to_string()});
        let mut stdout = std::io::stdout().lock();
        writeln!(stdout, "{ready}")
            .and_then(|_| stdout.flush())
            .map_err(|_| "lock_helper_failed")?;
        drop(stdout);
        let mut buffer = [0u8; 256];
        loop {
            match std::io::stdin().read(&mut buffer) {
                Ok(0) => break,
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                Err(_) => return Err("lock_helper_failed"),
            }
        }
        // Closing the handle releases this process's lock; the parent's own
        // handle keeps its no-sharing exclusion until it closes.
        drop(file);
        Ok(())
    }

    pub fn main() {
        let args = Args::parse();
        if let Err(code) = hold(&args) {
            println!("{}", json!({"schemaVersion":1,"type":"error","code":code}));
            std::process::exit(if code == "lock_busy" { 75 } else { 78 });
        }
    }
}

#[cfg(unix)]
fn main() {
    let args = Args::parse();
    if let Err(code) = hold(&args) {
        println!("{}", json!({"schemaVersion":1,"type":"error","code":code}));
        std::process::exit(if code == "lock_busy" { 75 } else { 78 });
    }
}
