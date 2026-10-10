//! Runtime-only holder of a lock shared with the owning Node file description.
//! Never unlock explicitly: the parent's retained descriptor protects pending IO.
use clap::Parser;
use serde_json::json;
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::PathBuf;

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
fn private(metadata: &std::fs::Metadata, directory: bool) -> bool {
    metadata.uid() == unsafe { libc::geteuid() }
        && metadata.mode() & 0o777 == if directory { 0o700 } else { 0o600 }
        && if directory {
            metadata.is_dir()
        } else {
            metadata.is_file() && metadata.nlink() == 1
        }
}
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
fn main() {
    let args = Args::parse();
    if let Err(code) = hold(&args) {
        println!("{}", json!({"schemaVersion":1,"type":"error","code":code}));
        std::process::exit(if code == "lock_busy" { 75 } else { 78 });
    }
}
