#![cfg(unix)]
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader};
use std::os::fd::AsRawFd;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, Stdio};

fn start(file: &File, path: &std::path::Path) -> (Child, serde_json::Value) {
    let fd = file.as_raw_fd();
    let mut command = Command::new(env!("CARGO_BIN_EXE_pu-point-guard-lock"));
    command.args([
        "--file",
        path.to_str().unwrap(),
        "--nonce",
        "fixture",
        "--owner-pid",
        &std::process::id().to_string(),
    ]);
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    // Only async-signal-safe descriptor operations run after fork.
    unsafe {
        command.pre_exec(move || {
            if libc::dup2(fd, 3) < 0 || libc::fcntl(3, libc::F_SETFD, 0) < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let mut child = command.spawn().unwrap();
    let mut line = String::new();
    BufReader::new(child.stdout.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    (child, serde_json::from_str(&line).unwrap())
}

#[test]
fn given_helper_crash_should_exclude_second_writer_until_parent_descriptor_closes() {
    // given: permanent private inode with inert old PID metadata
    let directory = tempfile::tempdir().unwrap();
    std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let path = directory.path().join("writer.lock");
    std::fs::write(&path, "old pid 999999\n").unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    let parent = OpenOptions::new()
        .read(true)
        .write(true)
        .mode(0o600)
        .open(&path)
        .unwrap();
    let inode = parent.metadata().unwrap().ino();
    let (mut first, ready) = start(&parent, &path);
    assert_eq!(ready["type"], "ready");
    // when: only the directly owned fixture helper crashes; parent still has pending IO ownership
    first.kill().unwrap();
    first.wait().unwrap();
    let contender = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&path)
        .unwrap();
    let (mut second, blocked) = start(&contender, &path);
    assert_eq!(blocked["code"], "lock_busy");
    second.wait().unwrap();
    drop(parent); // submitted owner IO has drained
    let (mut third, acquired) = start(&contender, &path);
    assert_eq!(acquired["type"], "ready");
    third.stdin.take();
    assert_eq!(third.wait().unwrap().code(), Some(0));
    assert_eq!(std::fs::metadata(&path).unwrap().ino(), inode);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "old pid 999999\n");
}

#[test]
fn given_public_lock_descriptor_should_fail_without_changing_or_removing_inode() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let path = directory.path().join("lock");
    std::fs::write(&path, "unchanged").unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
    let file = File::open(&path).unwrap();
    let (mut child, failure) = start(&file, &path);
    assert_eq!(failure["code"], "lock_private");
    child.wait().unwrap();
    assert_eq!(std::fs::read_to_string(path).unwrap(), "unchanged");
}
