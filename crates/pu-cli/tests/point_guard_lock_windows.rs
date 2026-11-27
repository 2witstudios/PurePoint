//! `pu-point-guard-lock` on Windows: the parent passes its open lock file by
//! handle inheritance (`PROC_THREAD_ATTRIBUTE_HANDLE_LIST`, `--handle`), the
//! helper takes `LockFileEx` on it after the private-state checks.
#![cfg(windows)]

use std::ffi::OsStr;
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader};
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::path::Path;
use std::ptr::{null, null_mut};

use windows_sys::Win32::Foundation::{
    CloseHandle, HANDLE, HANDLE_FLAG_INHERIT, SetHandleInformation, WAIT_OBJECT_0,
};
use windows_sys::Win32::Security::SECURITY_ATTRIBUTES;
use windows_sys::Win32::System::Pipes::CreatePipe;
use windows_sys::Win32::System::Threading::{
    CREATE_NO_WINDOW, CREATE_UNICODE_ENVIRONMENT, CreateProcessW, DeleteProcThreadAttributeList,
    EXTENDED_STARTUPINFO_PRESENT, GetExitCodeProcess, INFINITE, InitializeProcThreadAttributeList,
    LPPROC_THREAD_ATTRIBUTE_LIST, PROC_THREAD_ATTRIBUTE_HANDLE_LIST, PROCESS_INFORMATION,
    STARTF_USESTDHANDLES, STARTUPINFOEXW, TerminateProcess, UpdateProcThreadAttribute,
    WaitForSingleObject,
};

struct Helper {
    process: OwnedHandle,
    stdin: Option<File>,
    stdout: BufReader<File>,
}

impl Helper {
    fn line(&mut self) -> serde_json::Value {
        let mut line = String::new();
        self.stdout.read_line(&mut line).unwrap();
        serde_json::from_str(&line).unwrap_or_else(|_| panic!("not JSON: {line:?}"))
    }

    /// Close stdin (the release signal) and wait for the exit code.
    fn release(mut self) -> u32 {
        drop(self.stdin.take());
        self.wait()
    }

    fn wait(&mut self) -> u32 {
        let handle = self.process.as_raw_handle() as HANDLE;
        let mut code = 0u32;
        // SAFETY: a live process handle we own.
        unsafe {
            assert_eq!(WaitForSingleObject(handle, INFINITE), WAIT_OBJECT_0);
            GetExitCodeProcess(handle, &mut code);
        }
        code
    }

    fn kill(&mut self) {
        // SAFETY: a live process handle we own.
        unsafe { TerminateProcess(self.process.as_raw_handle() as HANDLE, 9) };
        self.wait();
    }
}

fn pipe() -> (File, File) {
    let attributes = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: null_mut(),
        bInheritHandle: 0,
    };
    let (mut read, mut write): (HANDLE, HANDLE) = (null_mut(), null_mut());
    // SAFETY: valid out-pointers; both handles are owned by the Files below.
    unsafe {
        assert_ne!(CreatePipe(&mut read, &mut write, &attributes, 0), 0);
        (
            File::from_raw_handle(read as _),
            File::from_raw_handle(write as _),
        )
    }
}

fn quote(arg: &str) -> String {
    if arg.contains([' ', '"']) {
        format!("\"{}\"", arg.replace('"', "\\\""))
    } else {
        arg.to_string()
    }
}

/// Start the helper with exactly the lock handle and its stdio inherited.
fn start(file: &File, path: &Path, handle_arg: Option<u64>, owner: u32) -> Helper {
    let (child_in, parent_in) = pipe();
    let (parent_out, child_out) = pipe();
    let nul = OpenOptions::new().write(true).open("NUL").unwrap();
    let handles: [HANDLE; 4] = [
        file.as_raw_handle() as HANDLE,
        child_in.as_raw_handle() as HANDLE,
        child_out.as_raw_handle() as HANDLE,
        nul.as_raw_handle() as HANDLE,
    ];
    for handle in handles {
        // SAFETY: our own handles, inheritable only so the list may name them.
        assert_ne!(
            unsafe { SetHandleInformation(handle, HANDLE_FLAG_INHERIT, HANDLE_FLAG_INHERIT) },
            0
        );
    }
    let binary = env!("CARGO_BIN_EXE_pu-point-guard-lock");
    let handle_value = handle_arg
        .unwrap_or(file.as_raw_handle() as u64)
        .to_string();
    let line = [
        quote(binary),
        "--file".into(),
        quote(path.to_str().unwrap()),
        "--nonce".into(),
        "fixture".into(),
        "--owner-pid".into(),
        owner.to_string(),
        "--handle".into(),
        handle_value,
    ]
    .join(" ");
    let mut command: Vec<u16> = OsStr::new(&line).encode_wide().chain([0]).collect();
    // SAFETY: the attribute list lives in `buffer` until deleted; every
    // pointer passed to CreateProcessW outlives the call.
    let process = unsafe {
        let mut size = 0usize;
        InitializeProcThreadAttributeList(null_mut(), 1, 0, &mut size);
        let mut buffer = vec![0u8; size];
        let attrs = buffer.as_mut_ptr() as LPPROC_THREAD_ATTRIBUTE_LIST;
        assert_ne!(InitializeProcThreadAttributeList(attrs, 1, 0, &mut size), 0);
        assert_ne!(
            UpdateProcThreadAttribute(
                attrs,
                0,
                PROC_THREAD_ATTRIBUTE_HANDLE_LIST as usize,
                handles.as_ptr() as *const _,
                std::mem::size_of_val(&handles),
                null_mut(),
                null(),
            ),
            0
        );
        let mut startup: STARTUPINFOEXW = std::mem::zeroed();
        startup.StartupInfo.cb = std::mem::size_of::<STARTUPINFOEXW>() as u32;
        startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
        startup.StartupInfo.hStdInput = handles[1];
        startup.StartupInfo.hStdOutput = handles[2];
        startup.StartupInfo.hStdError = handles[3];
        startup.lpAttributeList = attrs;
        let mut info: PROCESS_INFORMATION = std::mem::zeroed();
        let ok = CreateProcessW(
            null(),
            command.as_mut_ptr(),
            null(),
            null(),
            1,
            EXTENDED_STARTUPINFO_PRESENT | CREATE_NO_WINDOW | CREATE_UNICODE_ENVIRONMENT,
            null(),
            null(),
            &startup.StartupInfo,
            &mut info,
        );
        DeleteProcThreadAttributeList(attrs);
        assert_ne!(ok, 0, "{}", std::io::Error::last_os_error());
        CloseHandle(info.hThread);
        OwnedHandle::from_raw_handle(info.hProcess as _)
    };
    // The parent never passes these on again.
    for handle in &handles[..1] {
        // SAFETY: our own handle.
        unsafe { SetHandleInformation(*handle, HANDLE_FLAG_INHERIT, 0) };
    }
    Helper {
        process,
        stdin: Some(parent_in),
        stdout: BufReader::new(parent_out),
    }
}

fn me() -> u32 {
    std::process::id()
}

fn private_dir() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    pu_core::win_security::set_private_dacl(dir.path(), true).unwrap();
    dir
}

fn open(path: &Path) -> File {
    OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
        .unwrap()
}

#[test]
fn given_inherited_handle_should_lock_report_identity_and_exclude_a_second_holder() {
    let dir = private_dir();
    let path = dir.path().join("runtime.lock");
    std::fs::write(&path, "old pid 999999\n").unwrap();
    let first_file = open(&path);
    let mut first = start(&first_file, &path, None, me());
    let ready = first.line();
    assert_eq!(ready["type"], "ready", "{ready}");
    assert_eq!(ready["nonce"], "fixture");
    let (device, inode) = pu_core::private_fs::file_identity(&first_file).unwrap();
    assert_eq!(ready["device"], device.to_string());
    assert_eq!(ready["inode"], inode.to_string());
    // A second open of the same file (sharing allowed here) cannot take the lock.
    let second_file = open(&path);
    let mut second = start(&second_file, &path, None, me());
    assert_eq!(second.line()["code"], "lock_busy");
    assert_eq!(second.wait(), 75);
    assert_eq!(first.release(), 0);
    // Released on the holder's exit: the next holder succeeds.
    let mut third = start(&second_file, &path, None, me());
    assert_eq!(third.line()["type"], "ready");
    third.kill();
    let mut fourth = start(&second_file, &path, None, me());
    assert_eq!(
        fourth.line()["type"],
        "ready",
        "a killed holder's lock is gone"
    );
    assert_eq!(fourth.release(), 0);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "old pid 999999\n");
}

#[test]
fn given_shared_acl_or_junction_should_refuse_as_not_private() {
    let dir = private_dir();
    let path = dir.path().join("writer.lock");
    std::fs::write(&path, "").unwrap();
    pu_core::win_security::grant_for_test(&path, "WD").unwrap(); // Everyone
    let file = open(&path);
    let mut helper = start(&file, &path, None, me());
    assert_eq!(helper.line()["code"], "lock_private");
    assert_eq!(helper.wait(), 78);
    // A junction as the lock's directory is a reparse point: never followed.
    let real = dir.path().join("real");
    std::fs::create_dir(&real).unwrap();
    pu_core::win_security::set_private_dacl(&real, true).unwrap();
    let junction = dir.path().join("junction");
    let status = std::process::Command::new("cmd")
        .args(["/d", "/c", "mklink", "/J"])
        .arg(&junction)
        .arg(&real)
        .stdout(std::process::Stdio::null())
        .status()
        .unwrap();
    assert!(status.success());
    let through = junction.join("writer.lock");
    let file = open(&through);
    let mut helper = start(&file, &through, None, me());
    assert_eq!(helper.line()["code"], "lock_private");
    helper.wait();
}

#[test]
fn given_another_path_wrong_owner_or_bad_handle_should_refuse() {
    let dir = private_dir();
    let path = dir.path().join("a.lock");
    let other = dir.path().join("b.lock");
    std::fs::write(&other, "").unwrap();
    let file = open(&path);
    let mut helper = start(&file, &other, None, me());
    assert_eq!(helper.line()["code"], "lock_path_changed");
    helper.wait();
    let mut helper = start(&file, &path, None, me() + 1);
    assert_eq!(helper.line()["code"], "lock_helper_failed");
    helper.wait();
    // A handle value that names nothing inherited.
    let mut helper = start(&file, &path, Some(0x7ff0), me());
    assert_eq!(helper.line()["code"], "lock_helper_failed");
    helper.wait();
}
