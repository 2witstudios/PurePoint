//! Windows process introspection (WINAPP-051): Toolhelp32 for the process
//! tree, `GetProcessTimes` for start times and `NtQueryInformationProcess`
//! for command lines — never `ps` or `lsof`.

use std::collections::HashMap;
use std::os::windows::ffi::OsStringExt;
use std::time::{Duration, SystemTime};

use windows_sys::Wdk::System::Threading::{
    NtQueryInformationProcess, ProcessCommandLineInformation,
};
use windows_sys::Win32::Foundation::{
    CloseHandle, FILETIME, HANDLE, INVALID_HANDLE_VALUE, UNICODE_STRING,
};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows_sys::Win32::System::Threading::{
    GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
};

/// An owned process or snapshot handle.
struct Handle(HANDLE);

impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: the handle is valid and owned; it is closed exactly once.
        unsafe { CloseHandle(self.0) };
    }
}

fn open_process(pid: u32) -> Option<Handle> {
    // SAFETY: OpenProcess has no pointer arguments; a null result is an error.
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    (!handle.is_null()).then_some(Handle(handle))
}

/// `pid`'s creation time as a raw FILETIME (100 ns ticks since 1601).
pub(crate) fn creation_filetime(pid: u32) -> Option<u64> {
    let process = open_process(pid)?;
    let mut times = [FILETIME {
        dwLowDateTime: 0,
        dwHighDateTime: 0,
    }; 4];
    let [created, exited, kernel, user] = &mut times;
    // SAFETY: the handle is open with query rights; all four out-pointers are valid.
    let ok = unsafe { GetProcessTimes(process.0, created, exited, kernel, user) };
    (ok != 0).then(|| (u64::from(created.dwHighDateTime) << 32) | u64::from(created.dwLowDateTime))
}

/// FILETIME ticks between 1601-01-01 and the Unix epoch.
const UNIX_EPOCH_FILETIME: u64 = 116_444_736_000_000_000;

pub(crate) fn filetime_to_system_time(filetime: u64) -> Option<SystemTime> {
    let since_epoch = filetime.checked_sub(UNIX_EPOCH_FILETIME)?;
    SystemTime::UNIX_EPOCH.checked_add(Duration::from_nanos(since_epoch.checked_mul(100)?))
}

/// When `pid` started, or None if it is gone or not ours to query.
pub(crate) fn process_start_time(pid: u32) -> Option<SystemTime> {
    filetime_to_system_time(creation_filetime(pid)?)
}

/// Every `(pid, parent pid)` pair from one Toolhelp32 snapshot.
fn snapshot_parents() -> Option<Vec<(u32, u32)>> {
    // SAFETY: no pointer arguments; INVALID_HANDLE_VALUE is the error result.
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return None;
    }
    let snapshot = Handle(snapshot);
    // SAFETY: PROCESSENTRY32W is plain data; dwSize is set before use.
    let mut entry: PROCESSENTRY32W = unsafe { std::mem::zeroed() };
    entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
    let mut pairs = Vec::new();
    // SAFETY: `entry` is a valid, sized PROCESSENTRY32W for both calls.
    let mut more = unsafe { Process32FirstW(snapshot.0, &mut entry) } != 0;
    while more {
        pairs.push((entry.th32ProcessID, entry.th32ParentProcessID));
        // SAFETY: as above.
        more = unsafe { Process32NextW(snapshot.0, &mut entry) } != 0;
    }
    Some(pairs)
}

/// Parent→children map from `(pid, ppid)` pairs. Windows keeps a dead
/// parent's PID in its children, and that PID can be reused by a process
/// started later, so an edge is kept only when the parent is not younger
/// than the child (when both creation times are known). This also rules out
/// cycles, which would make descendant walks loop.
pub(crate) fn build_tree(
    pairs: &[(u32, u32)],
    created: impl Fn(u32) -> Option<u64>,
) -> HashMap<i32, Vec<i32>> {
    let times: HashMap<u32, Option<u64>> =
        pairs.iter().map(|&(pid, _)| (pid, created(pid))).collect();
    let mut children: HashMap<i32, Vec<i32>> = HashMap::new();
    for &(pid, ppid) in pairs {
        if pid == 0 || pid == ppid {
            continue;
        }
        let (Ok(child), Ok(parent)) = (i32::try_from(pid), i32::try_from(ppid)) else {
            continue;
        };
        let child_time = times.get(&pid).copied().flatten();
        let parent_time = times.get(&ppid).copied().flatten();
        if let (Some(c), Some(p)) = (child_time, parent_time)
            && p > c
        {
            continue;
        }
        children.entry(parent).or_default().push(child);
    }
    children
}

/// Parent→children map of every process.
pub(crate) fn process_tree() -> Option<HashMap<i32, Vec<i32>>> {
    Some(build_tree(&snapshot_parents()?, creation_filetime))
}

/// `pid`'s full command line (`ProcessCommandLineInformation`), or None if it
/// is gone or not ours to query.
pub(crate) fn process_command(pid: u32) -> Option<String> {
    let process = open_process(pid)?;
    let mut len = 0u32;
    // SAFETY: a zero-length query only reports the needed size in `len`.
    unsafe {
        NtQueryInformationProcess(
            process.0,
            ProcessCommandLineInformation,
            std::ptr::null_mut(),
            0,
            &mut len,
        )
    };
    let header = std::mem::size_of::<UNICODE_STRING>();
    if (len as usize) < header || len > 1 << 20 {
        return None;
    }
    // u64 storage keeps the UNICODE_STRING header aligned.
    let mut buf = vec![0u64; (len as usize).div_ceil(8)];
    // SAFETY: `buf` holds at least `len` bytes and is suitably aligned.
    let status = unsafe {
        NtQueryInformationProcess(
            process.0,
            ProcessCommandLineInformation,
            buf.as_mut_ptr().cast(),
            len,
            &mut len,
        )
    };
    if status < 0 {
        return None;
    }
    // SAFETY: on success the buffer starts with a UNICODE_STRING whose Buffer
    // points into `buf` and spans Length bytes.
    let text = unsafe {
        let s = &*buf.as_ptr().cast::<UNICODE_STRING>();
        if s.Buffer.is_null() {
            return Some(String::new());
        }
        std::slice::from_raw_parts(s.Buffer, usize::from(s.Length) / 2)
    };
    Some(
        std::ffi::OsString::from_wide(text)
            .to_string_lossy()
            .into_owned(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_reused_parent_pid_should_drop_the_stale_edge_and_any_cycle() {
        // 10 started before its "parent" 20 (a reused PID), and vice versa.
        let pairs = [(10, 20), (20, 10), (30, 10), (0, 0), (4, 0)];
        let times = |pid| match pid {
            10 => Some(100),
            20 => Some(200),
            30 => Some(300),
            _ => None,
        };
        let tree = build_tree(&pairs, times);
        assert_eq!(tree.get(&10), Some(&vec![20, 30]));
        assert_eq!(tree.get(&20), None);
        assert_eq!(tree.get(&0), Some(&vec![4]));
        assert!(!tree.values().flatten().any(|&p| p == 0));
    }

    #[test]
    fn given_spawned_child_should_appear_in_tree_with_command_and_start_time() {
        let before = SystemTime::now() - Duration::from_secs(2);
        let mut child = std::process::Command::new("cmd.exe")
            .args(["/d", "/c", "ping -n 5 127.0.0.1 >nul & rem pu-marker-3-7"])
            .stdout(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let tree = process_tree().unwrap();
        let ours = std::process::id() as i32;
        let found = tree
            .get(&ours)
            .is_some_and(|kids| kids.contains(&(child.id() as i32)));
        let command = process_command(child.id());
        let started = process_start_time(child.id());
        child.kill().ok();
        child.wait().ok();
        assert!(found, "child missing from Toolhelp32 tree");
        assert!(
            command
                .as_deref()
                .is_some_and(|c| c.contains("pu-marker-3-7")),
            "{command:?}"
        );
        assert!(started.is_some_and(|t| t >= before && t <= SystemTime::now()));
        assert!(process_command(u32::MAX - 3).is_none());
    }
}
