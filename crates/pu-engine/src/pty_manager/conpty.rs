//! Windows PTY host: one pseudoconsole, one suspended root process and one
//! kill-on-close Job Object per agent.
//!
//! The spawn sequence, the launch forms and the `cmd.exe` quoting are the ones
//! WIN-1.1 proved (`docs/research/windows-conpty.md`, `spikes/conpty`) and
//! `docs/product/windows-app.md` specifies (PTY host contract).

use std::collections::BTreeMap;
use std::ffi::c_void;
use std::fs::File;
use std::io::{self, Write};
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::path::{Path, PathBuf};
use std::ptr::{null, null_mut};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use tokio::sync::watch;
use windows_sys::Win32::Foundation::{
    FILETIME, HANDLE, INVALID_HANDLE_VALUE, WAIT_OBJECT_0, WAIT_TIMEOUT,
};
use windows_sys::Win32::System::Console::{
    COORD, ClosePseudoConsole, CreatePseudoConsole, HPCON, ResizePseudoConsole,
};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, IsProcessInJob, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
    SetInformationJobObject, TerminateJobObject,
};
use windows_sys::Win32::System::Pipes::CreatePipe;
use windows_sys::Win32::System::Threading::{
    CREATE_SUSPENDED, CREATE_UNICODE_ENVIRONMENT, CreateProcessW, DeleteProcThreadAttributeList,
    EXTENDED_STARTUPINFO_PRESENT, GetExitCodeProcess, GetProcessTimes, INFINITE,
    InitializeProcThreadAttributeList, LPPROC_THREAD_ATTRIBUTE_LIST, OpenProcess,
    PROC_THREAD_ATTRIBUTE_DESKTOP_APP_POLICY, PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE,
    PROCESS_INFORMATION, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE, PROCESS_TERMINATE,
    ResumeThread, STARTF_USESTDHANDLES, STARTUPINFOEXW, TerminateProcess,
    UpdateProcThreadAttribute, WaitForSingleObject,
};

use super::{PtyHandle, SpawnConfig, pump_output};
use crate::output_buffer::OutputBuffer;

/// `PROCESS_CREATION_DESKTOP_APP_BREAKAWAY_DISABLE_PROCESS_TREE`: without it the
/// MSIX (Store) pwsh creates its children outside the job (WIN-1.1 section 5).
const DESKTOP_APP_BREAKAWAY_DISABLE_PROCESS_TREE: u32 = 2;

/// How long the exit watcher waits for the reader to drain after the
/// pseudoconsole closes, so the final output lands before the exit is reported.
const READER_DRAIN_TIMEOUT: Duration = Duration::from_secs(2);

/// Exit code of a job kill: Unix's `128 + SIGKILL`, so the existing "killed"
/// handling matches.
pub(super) const KILLED_EXIT_CODE: u32 = 137;

/// Environment variables never passed to agents (WINAPP-007): `NO_COLOR` strips
/// Claude's dim placeholder and breaks `input_phase`; the others are nesting
/// and terminal markers of whatever launched the daemon.
fn is_stripped_env(upper: &str) -> bool {
    upper == "NO_COLOR"
        || upper == "CLAUDECODE"
        || upper.starts_with("CLAUDE_CODE_")
        || upper.starts_with("WT_")
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// The HPCON, closed exactly once by whichever of exit, kill or drop is first.
struct PseudoConsole(Mutex<Option<HPCON>>);

impl PseudoConsole {
    fn close(&self) {
        if let Some(hpc) = lock(&self.0).take() {
            // The reader thread keeps draining the output pipe, so this cannot
            // block on a full pipe; it returns once conhost has flushed.
            unsafe { ClosePseudoConsole(hpc) };
        }
    }
}

impl Drop for PseudoConsole {
    fn drop(&mut self) {
        self.close();
    }
}

/// One agent's pseudoconsole, root process and job.
pub(super) struct ConPty {
    console: Arc<PseudoConsole>,
    input: File,
    output: Mutex<Option<File>>,
    process: Arc<OwnedHandle>,
    /// Kill-on-close: dropping the last handle ends the whole tree, also when
    /// the daemon crashes (REQ-WINAPP-016).
    job: OwnedHandle,
    pid: u32,
    /// `(cols, rows)` last set; ConPTY has no `TIOCGWINSZ`.
    size: Mutex<(u16, u16)>,
}

impl PtyHandle for ConPty {
    fn write(&self, data: &[u8]) -> io::Result<()> {
        (&self.input).write_all(data)?;
        (&self.input).flush()
    }

    fn resize(&self, cols: u16, rows: u16) -> io::Result<()> {
        let console = lock(&self.console.0);
        if let Some(hpc) = *console {
            let hr = unsafe { ResizePseudoConsole(hpc, coord(cols, rows)) };
            if hr != 0 {
                return Err(io::Error::other(format!(
                    "ResizePseudoConsole failed: HRESULT {hr:#010x}"
                )));
            }
        }
        // A closed console (exited agent) still records the size, as an ioctl
        // on a Unix PTY whose child exited still succeeds.
        *lock(&self.size) = (cols, rows);
        Ok(())
    }

    fn size(&self) -> Option<(u16, u16)> {
        Some(*lock(&self.size))
    }

    fn reader(&self) -> io::Result<Box<dyn io::Read + Send>> {
        lock(&self.output)
            .take()
            .map(|f| Box::new(f) as Box<dyn io::Read + Send>)
            .ok_or_else(|| io::Error::other("the pseudoconsole output is already being read"))
    }

    fn pid(&self) -> u32 {
        self.pid
    }
}

impl Drop for ConPty {
    fn drop(&mut self) {
        // Close the console before the job handle (dropped after this) kills
        // the tree, so attached clients see the console go away first.
        self.console.close();
    }
}

impl ConPty {
    /// `ClosePseudoConsole`: console clients are closed and the reader
    /// reaches EOF. Idempotent.
    pub(super) fn close_console(&self) {
        self.console.close();
    }

    /// `TerminateJobObject(137)`: every process still in the job ends.
    pub(super) fn terminate_job(&self) {
        unsafe { TerminateJobObject(self.job.as_raw_handle(), KILLED_EXIT_CODE) };
    }

    /// The root's descendants now, opened so a later sweep cannot hit a
    /// reused pid.
    pub(super) fn descendants(&self) -> Vec<Process> {
        descendants(self.pid, creation_time(self.process.as_raw_handle()))
    }

    /// End every snapshotted descendant still alive outside the job (one that
    /// escaped it despite the desktop-app policy).
    pub(super) fn sweep(&self, snapshot: Vec<Process>) {
        sweep(snapshot, Some(self.job.as_raw_handle()));
    }
}

// --- process tree (Toolhelp32) ----------------------------------------------

/// A process opened while it was known to be a descendant. Holding the handle
/// keeps its pid from being reused.
pub(super) struct Process {
    pub(super) pid: u32,
    handle: OwnedHandle,
}

impl Process {
    pub(super) fn is_alive(&self) -> bool {
        unsafe { WaitForSingleObject(self.handle.as_raw_handle(), 0) == WAIT_TIMEOUT }
    }
}

fn open_process(pid: u32) -> Option<OwnedHandle> {
    let access = PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_TERMINATE | PROCESS_SYNCHRONIZE;
    let raw = unsafe { OpenProcess(access, 0, pid) };
    // SAFETY: a non-null OpenProcess result is a fresh handle we own.
    (!raw.is_null()).then(|| unsafe { OwnedHandle::from_raw_handle(raw) })
}

fn creation_time(process: HANDLE) -> Option<u64> {
    let zero = FILETIME {
        dwLowDateTime: 0,
        dwHighDateTime: 0,
    };
    let (mut created, mut exited, mut kernel, mut user) = (zero, zero, zero, zero);
    let ok = unsafe { GetProcessTimes(process, &mut created, &mut exited, &mut kernel, &mut user) };
    (ok != 0).then(|| (u64::from(created.dwHighDateTime) << 32) | u64::from(created.dwLowDateTime))
}

/// `(pid, parent pid, exe name)` of every process.
fn process_table() -> Vec<(u32, u32, String)> {
    let snap = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snap == INVALID_HANDLE_VALUE || snap.is_null() {
        return Vec::new();
    }
    // SAFETY: a valid snapshot handle we own.
    let snap = unsafe { OwnedHandle::from_raw_handle(snap) };
    let mut table = Vec::new();
    let mut entry: PROCESSENTRY32W = unsafe { std::mem::zeroed() };
    entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
    let mut more = unsafe { Process32FirstW(snap.as_raw_handle(), &mut entry) } != 0;
    while more {
        let len = entry.szExeFile.iter().position(|&c| c == 0).unwrap_or(0);
        let name = String::from_utf16_lossy(&entry.szExeFile[..len]);
        table.push((entry.th32ProcessID, entry.th32ParentProcessID, name));
        more = unsafe { Process32NextW(snap.as_raw_handle(), &mut entry) } != 0;
    }
    table
}

/// Every descendant of `root_pid` by parent chain. A child created before its
/// recorded parent is not its child (the parent pid was reused) and is skipped.
pub(super) fn descendants(root_pid: u32, root_created: Option<u64>) -> Vec<Process> {
    let table = process_table();
    let mut found = Vec::new();
    let mut queue = std::collections::VecDeque::from([(root_pid, root_created)]);
    while let Some((parent, parent_created)) = queue.pop_front() {
        for (pid, ppid, _) in &table {
            if *ppid != parent || *pid == parent || *pid == root_pid {
                continue;
            }
            let Some(handle) = open_process(*pid) else {
                continue;
            };
            let created = creation_time(handle.as_raw_handle());
            if let (Some(child), Some(parent)) = (created, parent_created)
                && child < parent
            {
                continue;
            }
            if found.iter().any(|p: &Process| p.pid == *pid) {
                continue;
            }
            queue.push_back((*pid, created));
            found.push(Process { pid: *pid, handle });
        }
    }
    found
}

/// Live descendants of `pid` with their exe names, for tests.
#[cfg(test)]
pub(super) fn named_descendants(pid: u32) -> Vec<(Process, String)> {
    let root = open_process(pid);
    let created = root.as_ref().and_then(|r| creation_time(r.as_raw_handle()));
    let table = process_table();
    descendants(pid, created)
        .into_iter()
        .map(|p| {
            let name = table
                .iter()
                .find(|(id, _, _)| *id == p.pid)
                .map(|(_, _, n)| n.clone())
                .unwrap_or_default();
            (p, name)
        })
        .collect()
}

/// Terminate each still-running process in `snapshot` that is not in `job`
/// (the job kill already ended those).
fn sweep(snapshot: Vec<Process>, job: Option<HANDLE>) {
    for p in snapshot {
        if !p.is_alive() {
            continue;
        }
        if let Some(job) = job {
            let mut in_job = 0;
            if unsafe { IsProcessInJob(p.handle.as_raw_handle(), job, &mut in_job) } != 0
                && in_job != 0
            {
                continue;
            }
        }
        unsafe { TerminateProcess(p.handle.as_raw_handle(), KILLED_EXIT_CODE) };
    }
}

/// Stop a process tree known only by its root pid (an agent that outlived its
/// daemon): there is no console to type into, so the root and its
/// descendants are terminated, the root first so it spawns nothing new.
pub(super) fn stop_process_tree(pid: u32) {
    let Some(root) = open_process(pid) else {
        return;
    };
    let tree = descendants(pid, creation_time(root.as_raw_handle()));
    unsafe {
        TerminateProcess(root.as_raw_handle(), KILLED_EXIT_CODE);
        WaitForSingleObject(root.as_raw_handle(), 3000);
    }
    sweep(tree, None);
}

fn coord(cols: u16, rows: u16) -> COORD {
    COORD {
        X: cols.min(i16::MAX as u16) as i16,
        Y: rows.min(i16::MAX as u16) as i16,
    }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn create_pipe() -> io::Result<(OwnedHandle, OwnedHandle)> {
    let (mut read, mut write): (HANDLE, HANDLE) = (null_mut(), null_mut());
    if unsafe { CreatePipe(&mut read, &mut write, null(), 0) } == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: CreatePipe returned two fresh handles we now own.
    unsafe {
        Ok((
            OwnedHandle::from_raw_handle(read),
            OwnedHandle::from_raw_handle(write),
        ))
    }
}

/// A proc-thread attribute list, deleted on drop.
struct AttributeList {
    buf: Vec<usize>,
}

impl AttributeList {
    fn new(count: u32) -> io::Result<Self> {
        let mut size = 0usize;
        unsafe { InitializeProcThreadAttributeList(null_mut(), count, 0, &mut size) };
        let mut buf = vec![0usize; size.div_ceil(std::mem::size_of::<usize>())];
        if unsafe { InitializeProcThreadAttributeList(buf.as_mut_ptr() as _, count, 0, &mut size) }
            == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(Self { buf })
    }

    fn as_ptr(&mut self) -> LPPROC_THREAD_ATTRIBUTE_LIST {
        self.buf.as_mut_ptr() as _
    }

    /// `value` must stay valid until the list is used by `CreateProcessW`.
    fn update(&mut self, attribute: u32, value: *const c_void, size: usize) -> io::Result<()> {
        if unsafe {
            UpdateProcThreadAttribute(
                self.as_ptr(),
                0,
                attribute as usize,
                value,
                size,
                null_mut(),
                null(),
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
}

impl Drop for AttributeList {
    fn drop(&mut self) {
        unsafe { DeleteProcThreadAttributeList(self.as_ptr()) };
    }
}

/// Spawn `config` under a new pseudoconsole and start its reader and exit
/// watcher threads. Output goes to `buffer`; the exit code goes to `exit_tx`.
pub(super) fn spawn(
    config: &SpawnConfig,
    buffer: Arc<OutputBuffer>,
    exit_tx: watch::Sender<Option<i32>>,
) -> io::Result<Arc<ConPty>> {
    let env = agent_environment(std::env::vars(), &config.env_remove, &config.env);
    let cwd = (!config.cwd.is_empty()).then(|| PathBuf::from(&config.cwd));
    let program = resolve_program(&config.command, &env, cwd.as_deref())?;
    let cmdline = command_line(&program, &config.args, &env)?;
    let env_block = env_block(&env)?;

    // 1. Pipes.
    let (in_read, in_write) = create_pipe()?;
    let (out_read, out_write) = create_pipe()?;

    // 2. Pseudoconsole (no passthrough flag: WIN-1.1 found no benefit).
    let mut hpc: HPCON = 0;
    let hr = unsafe {
        CreatePseudoConsole(
            coord(config.cols, config.rows),
            in_read.as_raw_handle(),
            out_write.as_raw_handle(),
            0,
            &mut hpc,
        )
    };
    if hr != 0 {
        return Err(io::Error::other(format!(
            "CreatePseudoConsole failed: HRESULT {hr:#010x}"
        )));
    }
    let console = Arc::new(PseudoConsole(Mutex::new(Some(hpc))));

    // 3. Attributes: the HPCON value itself, and the desktop-app policy.
    let mut attrs = AttributeList::new(2)?;
    attrs.update(
        PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE,
        hpc as *const c_void,
        std::mem::size_of::<HPCON>(),
    )?;
    let policy: u32 = DESKTOP_APP_BREAKAWAY_DISABLE_PROCESS_TREE;
    attrs.update(
        PROC_THREAD_ATTRIBUTE_DESKTOP_APP_POLICY,
        &policy as *const u32 as *const c_void,
        std::mem::size_of::<u32>(),
    )?;

    // 4. Null std handles, so a daemon with redirected stdio leaks nothing.
    let mut si: STARTUPINFOEXW = unsafe { std::mem::zeroed() };
    si.StartupInfo.cb = std::mem::size_of::<STARTUPINFOEXW>() as u32;
    si.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
    si.StartupInfo.hStdInput = null_mut();
    si.StartupInfo.hStdOutput = null_mut();
    si.StartupInfo.hStdError = null_mut();
    si.lpAttributeList = attrs.as_ptr();

    // 5. Suspended, no inherited handles.
    let mut cmd_w = wide(&cmdline);
    let cwd_w = cwd.as_ref().map(|c| wide(&c.to_string_lossy()));
    let mut pi: PROCESS_INFORMATION = unsafe { std::mem::zeroed() };
    let ok = unsafe {
        CreateProcessW(
            null(),
            cmd_w.as_mut_ptr(),
            null(),
            null(),
            0,
            EXTENDED_STARTUPINFO_PRESENT | CREATE_SUSPENDED | CREATE_UNICODE_ENVIRONMENT,
            env_block.as_ptr() as *const c_void,
            cwd_w.as_ref().map_or(null(), |w| w.as_ptr()),
            &si.StartupInfo,
            &mut pi,
        )
    };
    if ok == 0 {
        let e = io::Error::last_os_error();
        return Err(io::Error::new(
            e.kind(),
            format!("failed to start {}: {e}", program.display()),
        ));
    }
    drop(attrs);
    // SAFETY: CreateProcessW returned fresh process and thread handles.
    let process = Arc::new(unsafe { OwnedHandle::from_raw_handle(pi.hProcess) });
    let thread = unsafe { OwnedHandle::from_raw_handle(pi.hThread) };

    // 6. ConPTY holds its own duplicates of the child's ends.
    drop(in_read);
    drop(out_write);

    // 7. Job, then resume. On any failure the suspended child never runs.
    let fail = |e: io::Error| {
        unsafe { TerminateProcess(process.as_raw_handle(), 1) };
        e
    };
    let job = create_kill_on_close_job().map_err(fail)?;
    if unsafe { AssignProcessToJobObject(job.as_raw_handle(), process.as_raw_handle()) } == 0 {
        return Err(fail(io::Error::last_os_error()));
    }
    if unsafe { ResumeThread(thread.as_raw_handle()) } == u32::MAX {
        return Err(fail(io::Error::last_os_error()));
    }
    drop(thread);

    let pty = Arc::new(ConPty {
        console: console.clone(),
        input: File::from(in_write),
        output: Mutex::new(Some(File::from(out_read))),
        process: process.clone(),
        job,
        pid: pi.dwProcessId,
        size: Mutex::new((config.cols, config.rows)),
    });

    // 8. Dedicated reader thread until EOF.
    let reader = pty.reader()?;
    let (drained_tx, drained_rx) = std::sync::mpsc::channel::<()>();
    std::thread::Builder::new()
        .name(format!("pu-conpty-read-{}", pi.dwProcessId))
        .spawn(move || {
            pump_output(reader, &buffer);
            let _ = drained_tx.send(());
        })?;

    // Exit watcher: report the code, then close the console so the reader
    // reaches EOF (REQ-WINAPP-014). It holds no reference to the job, so
    // dropping the handle still kills the tree.
    std::thread::Builder::new()
        .name(format!("pu-conpty-wait-{}", pi.dwProcessId))
        .spawn(move || {
            let code = wait_exit_code(&process);
            console.close();
            let _ = drained_rx.recv_timeout(READER_DRAIN_TIMEOUT);
            exit_tx.send(code).ok();
        })?;

    Ok(pty)
}

fn create_kill_on_close_job() -> io::Result<OwnedHandle> {
    let raw = unsafe { CreateJobObjectW(null(), null()) };
    if raw.is_null() {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: CreateJobObjectW returned a fresh handle.
    let job = unsafe { OwnedHandle::from_raw_handle(raw) };
    let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
    info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    if unsafe {
        SetInformationJobObject(
            job.as_raw_handle(),
            JobObjectExtendedLimitInformation,
            &info as *const _ as *const c_void,
            std::mem::size_of_val(&info) as u32,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(job)
}

/// Block until `process` exits; its code as `i32` (`0xC000013A` is
/// `-1073741510`, a job kill with 137 is 137).
fn wait_exit_code(process: &OwnedHandle) -> Option<i32> {
    if unsafe { WaitForSingleObject(process.as_raw_handle(), INFINITE) } != WAIT_OBJECT_0 {
        return None;
    }
    let mut code = 0u32;
    (unsafe { GetExitCodeProcess(process.as_raw_handle(), &mut code) } != 0).then_some(code as i32)
}

// --- environment -----------------------------------------------------------

/// The agent's environment: the daemon's, minus `remove`, plus `set`, minus
/// the stripped variables. Keys compare case-insensitively, as Windows does;
/// the result is sorted the way `CreateProcessW` expects.
fn agent_environment(
    base: impl IntoIterator<Item = (String, String)>,
    remove: &[String],
    set: &[(String, String)],
) -> Vec<(String, String)> {
    let mut vars: BTreeMap<String, (String, String)> = BTreeMap::new();
    for (k, v) in base {
        vars.insert(k.to_uppercase(), (k, v));
    }
    for k in remove {
        vars.remove(&k.to_uppercase());
    }
    for (k, v) in set {
        vars.insert(k.to_uppercase(), (k.clone(), v.clone()));
    }
    vars.retain(|upper, _| !is_stripped_env(upper));
    vars.into_values().collect()
}

fn env_get<'a>(env: &'a [(String, String)], key: &str) -> Option<&'a str> {
    env.iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(key))
        .map(|(_, v)| v.as_str())
}

/// `CREATE_UNICODE_ENVIRONMENT` block: `k=v\0` entries, then a final `\0`.
fn env_block(env: &[(String, String)]) -> io::Result<Vec<u16>> {
    let mut block = Vec::new();
    for (k, v) in env {
        if k.is_empty() || k[1..].contains('=') || k.contains('\0') || v.contains('\0') {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("invalid environment variable {k:?}"),
            ));
        }
        block.extend(k.encode_utf16());
        block.push(u16::from(b'='));
        block.extend(v.encode_utf16());
        block.push(0);
    }
    if block.is_empty() {
        block.push(0);
    }
    block.push(0);
    Ok(block)
}

// --- program resolution and launch forms -------------------------------------

/// True for an existing non-directory, including app execution aliases (the
/// Store pwsh), whose reparse point `metadata` cannot follow.
fn is_file(p: &Path) -> bool {
    match std::fs::metadata(p) {
        Ok(m) => m.is_file(),
        Err(_) => std::fs::symlink_metadata(p).is_ok_and(|m| !m.is_dir()),
    }
}

fn extension_lower(p: &Path) -> String {
    p.extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default()
}

/// Resolve `command` the way a shell would: a path is used as given (relative
/// to `cwd`), a bare name is searched on the agent's `PATH`, then the daemon's,
/// trying the name itself and then each `PATHEXT` extension.
fn resolve_program(
    command: &str,
    env: &[(String, String)],
    cwd: Option<&Path>,
) -> io::Result<PathBuf> {
    let pathext = env_get(env, "PATHEXT").unwrap_or(".COM;.EXE;.BAT;.CMD");
    let exts: Vec<&str> = pathext.split(';').filter(|e| !e.is_empty()).collect();
    let with_ext = |base: &Path| -> Option<PathBuf> {
        if base.extension().is_some() && is_file(base) {
            return Some(base.to_path_buf());
        }
        exts.iter().find_map(|ext| {
            let mut name = base.as_os_str().to_owned();
            name.push(ext);
            let p = PathBuf::from(name);
            is_file(&p).then_some(p)
        })
    };
    let not_found = || {
        io::Error::new(
            io::ErrorKind::NotFound,
            format!("program not found: {command}"),
        )
    };
    if command.is_empty() {
        return Err(not_found());
    }
    let path = Path::new(command);
    if path.is_absolute() || command.contains(['\\', '/']) {
        let path = match cwd {
            Some(cwd) if path.is_relative() => cwd.join(path),
            _ => path.to_path_buf(),
        };
        return with_ext(&path).ok_or_else(not_found);
    }
    let daemon_path = std::env::var_os("PATH").unwrap_or_default();
    let agent_path = env_get(env, "PATH").map(std::ffi::OsString::from);
    agent_path
        .iter()
        .flat_map(std::env::split_paths)
        .chain(std::env::split_paths(&daemon_path))
        .filter(|dir| !dir.as_os_str().is_empty())
        .find_map(|dir| with_ext(&dir.join(command)))
        .ok_or_else(not_found)
}

/// The `CreateProcessW` command line for `program` and `args` (launch-form
/// table in the PTY host contract).
fn command_line(program: &Path, args: &[String], env: &[(String, String)]) -> io::Result<String> {
    let ext = extension_lower(program);
    if ext == "cmd" || ext == "bat" {
        if let Some(target) = forwarder_target(program, env) {
            return msvc_command_line(&target, args);
        }
        let comspec = env_get(env, "COMSPEC")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                let root = env_get(env, "SystemRoot").unwrap_or(r"C:\Windows");
                Path::new(root).join("System32").join("cmd.exe")
            });
        let mut inner = format!("\"{}\"", program.display());
        for arg in args {
            inner.push(' ');
            inner.push_str(&quote_cmd_arg(arg)?);
        }
        return Ok(format!("\"{}\" /d /s /c \"{inner}\"", comspec.display()));
    }
    msvc_command_line(program, args)
}

fn msvc_command_line(program: &Path, args: &[String]) -> io::Result<String> {
    // argv[0] is read up to the next quote, with no backslash escaping.
    let mut line = format!("\"{}\"", program.display());
    for arg in args {
        line.push(' ');
        line.push_str(&quote_msvc_arg(arg)?);
    }
    Ok(line)
}

fn invalid_arg(what: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, what.to_string())
}

/// MSVC CRT (`CommandLineToArgvW`) quoting.
fn quote_msvc_arg(arg: &str) -> io::Result<String> {
    if arg.contains('\0') {
        return Err(invalid_arg("an argument contains NUL"));
    }
    if !arg.is_empty() && !arg.contains([' ', '\t', '\n', '\x0b', '"']) {
        return Ok(arg.to_string());
    }
    let mut out = String::from("\"");
    let mut backslashes = 0;
    for c in arg.chars() {
        match c {
            '\\' => backslashes += 1,
            '"' => {
                out.extend(std::iter::repeat_n('\\', backslashes * 2 + 1));
                backslashes = 0;
                out.push('"');
            }
            c => {
                out.extend(std::iter::repeat_n('\\', backslashes));
                backslashes = 0;
                out.push(c);
            }
        }
    }
    out.extend(std::iter::repeat_n('\\', backslashes * 2));
    out.push('"');
    Ok(out)
}

/// One argument on a `cmd.exe /d /s /c "…"` line (WIN-1.1 section 3). `"`
/// becomes `""` (never CRT-style `\"`, which is injectable) and `%` becomes
/// `%%cd:~,%`, which expands to nothing, so `%PATH%` arrives literally.
fn quote_cmd_arg(arg: &str) -> io::Result<String> {
    if arg.contains(['\r', '\n', '\0']) {
        return Err(invalid_arg(
            "an argument for a .cmd/.bat agent contains a newline or NUL",
        ));
    }
    let needs = arg.is_empty()
        || arg.contains([
            ' ', '\t', '"', '&', '|', '<', '>', '^', '(', ')', '%', '!', ',', ';', '=',
        ]);
    if !needs {
        return Ok(arg.to_string());
    }
    let mut out = String::from("\"");
    let mut backslashes = 0;
    for c in arg.chars() {
        match c {
            '\\' => backslashes += 1,
            '"' => {
                out.extend(std::iter::repeat_n('\\', backslashes * 2));
                backslashes = 0;
                out.push_str("\"\"");
            }
            '%' => {
                out.extend(std::iter::repeat_n('\\', backslashes));
                backslashes = 0;
                out.push_str("%%cd:~,%");
            }
            c => {
                out.extend(std::iter::repeat_n('\\', backslashes));
                backslashes = 0;
                out.push(c);
            }
        }
    }
    out.extend(std::iter::repeat_n('\\', backslashes * 2));
    out.push('"');
    Ok(out)
}

/// The exe a forwarder shim runs: a `.cmd`/`.bat` whose only command is
/// `@"<exe>" %*` (optionally with `@echo off` and `setlocal`/`endlocal`), with
/// `%~dp0` and `%VAR%` expanded, naming an existing `.exe`.
fn forwarder_target(shim: &Path, env: &[(String, String)]) -> Option<PathBuf> {
    let text = std::fs::read_to_string(shim).ok()?;
    parse_forwarder(&text, shim.parent()?, env).filter(|exe| is_file(exe))
}

fn parse_forwarder(text: &str, shim_dir: &Path, env: &[(String, String)]) -> Option<PathBuf> {
    let mut command = None;
    for line in text.lines() {
        let line = line.trim();
        let bare = line.strip_prefix('@').unwrap_or(line).to_ascii_lowercase();
        if line.is_empty() || matches!(bare.as_str(), "echo off" | "setlocal" | "endlocal") {
            continue;
        }
        if command.replace(line).is_some() {
            return None;
        }
    }
    let rest = command?;
    let rest = rest.strip_prefix('@').unwrap_or(rest);
    let (exe, tail) = rest.strip_prefix('"')?.split_once('"')?;
    if tail.trim() != "%*" {
        return None;
    }
    let exe = PathBuf::from(expand_batch_vars(exe, shim_dir, env)?);
    (extension_lower(&exe) == "exe" && exe.is_absolute()).then_some(exe)
}

/// Expand `%~dp0` (the shim's directory, with a trailing `\`) and `%NAME%`.
/// Anything else with a `%` is not a simple forwarder.
fn expand_batch_vars(s: &str, shim_dir: &Path, env: &[(String, String)]) -> Option<String> {
    let mut out = String::new();
    let mut rest = s;
    while let Some(i) = rest.find('%') {
        out.push_str(&rest[..i]);
        rest = &rest[i + 1..];
        if rest.len() >= 4 && rest[..4].eq_ignore_ascii_case("~dp0") {
            let dir = shim_dir.to_string_lossy();
            out.push_str(&dir);
            if !dir.ends_with('\\') {
                out.push('\\');
            }
            rest = &rest[4..];
            continue;
        }
        let end = rest.find('%')?;
        out.push_str(env_get(env, &rest[..end]).filter(|_| end > 0)?);
        rest = &rest[end + 1..];
    }
    out.push_str(rest);
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    fn assert_same(got: PathBuf, want: PathBuf) {
        let norm = |p: &Path| p.to_string_lossy().to_lowercase().replace(r"\.\", r"\");
        assert_eq!(norm(&got), norm(&want));
    }

    #[test]
    fn given_cmd_quoting_cases_should_match_win_1_1_rules() {
        let q = |s| quote_cmd_arg(s).unwrap();
        assert_eq!(q("plain"), "plain");
        assert_eq!(q(""), "\"\"");
        assert_eq!(q("say \"hi & bye\""), "\"say \"\"hi & bye\"\"\"");
        assert_eq!(q("x\" & echo INJECTED & "), "\"x\"\" & echo INJECTED & \"");
        assert_eq!(q(r"C:\dir with space\"), r#""C:\dir with space\\""#);
        assert_eq!(q(r#"bs\"q"#), r#""bs\\""q""#);
        assert_eq!(q("100%"), "\"100%%cd:~,%\"");
        assert_eq!(q("%PATH%"), "\"%%cd:~,%PATH%%cd:~,%\"");
        assert!(quote_cmd_arg("a\nb").is_err());
        assert!(quote_cmd_arg("a\0b").is_err());
    }

    #[test]
    fn given_msvc_quoting_should_round_trip_backslashes_and_quotes() {
        let q = |s| quote_msvc_arg(s).unwrap();
        assert_eq!(q("plain"), "plain");
        assert_eq!(q(""), "\"\"");
        assert_eq!(q("a b"), "\"a b\"");
        assert_eq!(q(r#"a"b"#), r#""a\"b""#);
        assert_eq!(q(r"C:\x y\"), r#""C:\x y\\""#);
        assert_eq!(q(r"C:\no\space"), r"C:\no\space");
    }

    #[test]
    fn given_shim_command_line_should_use_cmd_with_d_s_c() {
        let dir = tempfile::tempdir().unwrap();
        let shim = dir.path().join("my tool.cmd");
        std::fs::write(&shim, "@echo off\r\nnode \"%~dp0\\cli.js\" %*\r\n").unwrap();
        let e = env(&[("COMSPEC", r"C:\Windows\system32\cmd.exe")]);
        let line = command_line(&shim, &["a b".into(), "x&y".into()], &e).unwrap();
        assert_eq!(
            line,
            format!(
                "\"C:\\Windows\\system32\\cmd.exe\" /d /s /c \"\"{}\" \"a b\" \"x&y\"\"",
                shim.display()
            )
        );
        assert!(command_line(&shim, &["two\nlines".into()], &e).is_err());
    }

    #[test]
    fn given_forwarder_shim_should_launch_target_exe_directly() {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("bin dir");
        std::fs::create_dir(&bin).unwrap();
        let exe = bin.join("codex-x86_64.exe");
        std::fs::write(&exe, b"MZ").unwrap();
        let shim = dir.path().join("codex.cmd");
        std::fs::write(&shim, "@\"%BASE%\\bin dir\\codex-x86_64.exe\" %*\r\n").unwrap();
        let e = env(&[("base", &dir.path().to_string_lossy())]);
        let line = command_line(&shim, &["--flag".into(), "a b".into()], &e).unwrap();
        assert_eq!(line, format!("\"{}\" --flag \"a b\"", exe.display()));

        // %~dp0 and the setlocal/echo-off wrapper are accepted.
        std::fs::write(
            &shim,
            "@echo off\nsetlocal\n\"%~dp0bin dir\\codex-x86_64.exe\" %*\nendlocal\n",
        )
        .unwrap();
        assert_eq!(forwarder_target(&shim, &e), Some(exe.clone()));
    }

    #[test]
    fn given_non_forwarder_shims_should_not_resolve_a_target() {
        let d = Path::new(r"C:\shims");
        let e = env(&[]);
        assert_eq!(parse_forwarder("@\"%NOPE%\\x.exe\" %*", d, &e), None);
        assert_eq!(parse_forwarder("@\"C:\\x.exe\" --fixed %*", d, &e), None);
        assert_eq!(
            parse_forwarder("@\"C:\\x.exe\" %*\n@echo done", d, &e),
            None
        );
        assert_eq!(parse_forwarder("@\"C:\\x.js\" %*", d, &e), None);
        assert_eq!(
            parse_forwarder("@\"%~dp0x.exe\" %*", d, &e),
            Some(PathBuf::from(r"C:\shims\x.exe"))
        );
    }

    #[test]
    fn given_daemon_env_should_strip_no_color_and_nesting_markers() {
        let base = env(&[
            ("Path", r"C:\a"),
            ("NO_COLOR", "1"),
            ("ClaudeCode", "1"),
            ("CLAUDE_CODE_ENTRYPOINT", "cli"),
            ("WT_SESSION", "x"),
            ("KEEP", "yes"),
            ("DROP_ME", "x"),
        ]);
        let out = agent_environment(
            base,
            &["drop_me".into()],
            &env(&[("PATH", r"C:\b"), ("TERM", "xterm-256color")]),
        );
        assert_eq!(
            out,
            env(&[
                ("KEEP", "yes"),
                ("PATH", r"C:\b"),
                ("TERM", "xterm-256color")
            ])
        );
        let block = env_block(&out).unwrap();
        assert_eq!(&block[block.len() - 2..], &[0, 0]);
        assert_eq!(env_block(&[]).unwrap(), vec![0, 0]);
    }

    #[test]
    fn given_bare_name_should_resolve_with_pathext_including_cmd_only_agents() {
        let dir = tempfile::tempdir().unwrap();
        let spaced = dir.path().join("dir with space");
        std::fs::create_dir(&spaced).unwrap();
        std::fs::write(spaced.join("agent.cmd"), "@echo hi\r\n").unwrap();
        std::fs::write(spaced.join("tool.exe"), b"MZ").unwrap();
        let path = std::env::join_paths([Path::new(r"C:\does\not\exist"), &spaced]).unwrap();
        let e = env(&[
            ("PATH", &path.to_string_lossy()),
            ("PATHEXT", ".COM;.EXE;.BAT;.CMD"),
        ]);
        assert_same(
            resolve_program("agent", &e, None).unwrap(),
            spaced.join("agent.cmd"),
        );
        assert_same(
            resolve_program("tool", &e, None).unwrap(),
            spaced.join("tool.exe"),
        );
        assert_same(
            resolve_program("tool.exe", &e, None).unwrap(),
            spaced.join("tool.exe"),
        );
        assert_same(
            resolve_program(r".\agent", &e, Some(&spaced)).unwrap(),
            spaced.join(r".\agent.cmd"),
        );
        let err = resolve_program("no-such-agent-xyz", &e, None).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::NotFound);
    }

    #[test]
    fn given_empty_agent_path_should_fall_back_to_the_daemon_path() {
        let e = env(&[("PATH", "")]);
        let cmd = resolve_program("cmd", &e, None).unwrap();
        assert!(
            cmd.to_string_lossy()
                .to_ascii_lowercase()
                .ends_with("cmd.exe")
        );
    }

    // --- spawned through the real ConPTY ---------------------------------

    use crate::pty_manager::{AgentHandle, NativePtyHost};

    fn config(command: &str, args: &[&str], cwd: &Path) -> SpawnConfig {
        SpawnConfig {
            command: command.into(),
            args: args.iter().map(|a| a.to_string()).collect(),
            cwd: cwd.display().to_string(),
            env: vec![],
            env_remove: vec![],
            cols: 200,
            rows: 30,
        }
    }

    fn screen_text(handle: &AgentHandle) -> String {
        let (rows, cols) = handle.output_buffer.terminal_size();
        let mut parser = vt100::Parser::new(rows, cols, 0);
        parser.process(&handle.output_buffer.read_all());
        parser.screen().contents()
    }

    async fn wait_for_screen(handle: &AgentHandle, needle: &str, timeout: Duration) -> String {
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            let text = screen_text(handle);
            if text.contains(needle) || tokio::time::Instant::now() >= deadline {
                return text;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    async fn wait_exit(handle: &AgentHandle) -> Option<i32> {
        let mut rx = handle.exit_rx.clone();
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                if let Some(code) = *rx.borrow_and_update() {
                    return Some(code);
                }
                if rx.changed().await.is_err() {
                    return None;
                }
            }
        })
        .await
        .expect("agent did not exit")
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn given_cmd_should_stream_output_and_report_exit_code() {
        let dir = tempfile::tempdir().unwrap();
        let handle = NativePtyHost::new()
            .spawn(config(
                "cmd",
                &["/d", "/c", "echo pu-marker-1& exit 42"],
                dir.path(),
            ))
            .await
            .unwrap();
        assert!(handle.pid > 0);
        assert_eq!(wait_exit(&handle).await, Some(42));
        // The exit is reported only after the reader drained to EOF.
        assert!(screen_text(&handle).contains("pu-marker-1"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn given_env_and_cwd_should_apply_them_without_no_color() {
        let dir = tempfile::tempdir().unwrap();
        let mut cfg = config(
            "cmd",
            &[
                "/d",
                "/c",
                "cd& echo var=%PU_TEST_VAR%& if defined NO_COLOR (echo color=stripped-not) else (echo color=stripped)",
            ],
            dir.path(),
        );
        cfg.env = vec![
            ("PU_TEST_VAR".into(), "hello-env".into()),
            ("NO_COLOR".into(), "1".into()),
        ];
        let handle = NativePtyHost::new().spawn(cfg).await.unwrap();
        assert_eq!(wait_exit(&handle).await, Some(0));
        let text = screen_text(&handle);
        let cwd = dunce_like(dir.path());
        assert!(text.to_lowercase().contains(&cwd.to_lowercase()), "{text}");
        assert!(text.contains("var=hello-env"), "{text}");
        assert!(
            text.contains("color=stripped\n") || text.ends_with("color=stripped"),
            "{text}"
        );
    }

    fn dunce_like(p: &Path) -> String {
        p.to_string_lossy().trim_start_matches(r"\?\").to_string()
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn given_written_input_should_reach_the_console() {
        let dir = tempfile::tempdir().unwrap();
        let host = NativePtyHost::new();
        let handle = host
            .spawn(config("cmd", &["/d", "/q", "/k"], dir.path()))
            .await
            .unwrap();
        host.write_chunked_submit(&handle.pty(), b"echo pu-typed-%OS%")
            .await
            .unwrap();
        let text = wait_for_screen(&handle, "pu-typed-Windows_NT", Duration::from_secs(10)).await;
        assert!(text.contains("pu-typed-Windows_NT"), "{text}");
        host.write_input(&handle, b"exit 7\r").await.unwrap();
        assert_eq!(wait_exit(&handle).await, Some(7));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn given_cmd_shim_should_run_through_cmd_without_injection() {
        let dir = tempfile::tempdir().unwrap();
        let shim = dir.path().join("echo args.cmd");
        std::fs::write(&shim, "@echo shim-ran [%1] [%2]\r\n").unwrap();
        let handle = NativePtyHost::new()
            .spawn(config(
                &shim.display().to_string(),
                &["a&echo INJECTED", "100%"],
                dir.path(),
            ))
            .await
            .unwrap();
        assert_eq!(wait_exit(&handle).await, Some(0));
        let text = screen_text(&handle);
        assert!(
            text.contains(r#"shim-ran ["a&echo INJECTED"] ["100%"]"#),
            "{text}"
        );
        assert!(!text.lines().any(|l| l.trim() == "INJECTED"), "{text}");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn given_resize_should_resize_the_console_and_report_the_size() {
        let dir = tempfile::tempdir().unwrap();
        let host = NativePtyHost::new();
        let handle = host
            .spawn(config(
                "pwsh",
                &[
                    "-NoProfile",
                    "-NoLogo",
                    "-Command",
                    "while ($true) { $s = $Host.UI.RawUI.WindowSize; Write-Output ('SIZE=' + $s.Width + 'x' + $s.Height); Start-Sleep -Milliseconds 200 }",
                ],
                dir.path(),
            ))
            .await
            .unwrap();
        let text = wait_for_screen(&handle, "SIZE=200x30", Duration::from_secs(20)).await;
        assert!(text.contains("SIZE=200x30"), "{text}");
        host.resize(&handle, 120, 40).await.unwrap();
        handle.output_buffer.resize_terminal(40, 120);
        assert_eq!(handle.pty().size(), Some((120, 40)));
        let text = wait_for_screen(&handle, "SIZE=120x40", Duration::from_secs(10)).await;
        assert!(text.contains("SIZE=120x40"), "{text}");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn given_generic_agent_kill_should_close_console_and_return_within_grace() {
        let dir = tempfile::tempdir().unwrap();
        let host = NativePtyHost::new();
        let handle = host
            .spawn(config("cmd", &["/d", "/q", "/k"], dir.path()))
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(300)).await;
        let started = std::time::Instant::now();
        let state = host.kill(&handle, Duration::from_secs(2)).await.unwrap();
        assert!(
            started.elapsed() < Duration::from_secs(3),
            "{:?}",
            started.elapsed()
        );
        // ClosePseudoConsole ended cmd before the job kill was needed.
        assert!(state.exit_code.is_some());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn given_claude_agent_kill_should_send_two_interrupts_before_closing() {
        // A stand-in that, like Claude, exits only on the second ^C.
        let dir = tempfile::tempdir().unwrap();
        let script = "[Console]::TreatControlCAsInput = $true; $n = 0; \
            while ($true) { $k = [Console]::ReadKey($true); \
            if ($k.Key -eq 'C' -and $k.Modifiers -band 4) { $n++; Write-Output \"ctrl-c $n\"; if ($n -ge 2) { exit 0 } } }";
        let host = NativePtyHost::new();
        let mut handle = host
            .spawn(config(
                "pwsh",
                &["-NoProfile", "-NoLogo", "-Command", script],
                dir.path(),
            ))
            .await
            .unwrap();
        handle.agent_type = "claude".into();
        tokio::time::sleep(Duration::from_secs(2)).await;
        let state = host.kill(&handle, Duration::from_secs(5)).await.unwrap();
        assert_eq!(state.exit_code, Some(0), "{}", screen_text(&handle));
        assert!(screen_text(&handle).contains("ctrl-c 2"));
    }
}
