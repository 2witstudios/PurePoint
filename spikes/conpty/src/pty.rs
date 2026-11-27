//! Minimal ConPTY host: CreatePseudoConsole + CreateProcessW(CREATE_SUSPENDED,
//! PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE) + Job Object(KILL_ON_JOB_CLOSE) + resume.

use std::ffi::c_void;
use std::fs::File;
use std::io::{Read, Write};
use std::os::windows::io::FromRawHandle;
use std::ptr::{null, null_mut};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::System::Console::*;
use windows_sys::Win32::System::JobObjects::*;
use windows_sys::Win32::System::Pipes::CreatePipe;
use windows_sys::Win32::System::Threading::*;

pub const PSEUDOCONSOLE_PASSTHROUGH_MODE: u32 = 0x8;

pub struct Pty {
    pub hpc: Option<HPCON>,
    input: File,
    pub output: Arc<Mutex<Vec<u8>>>,
    pub process: HANDLE,
    pub pid: u32,
    pub job: HANDLE,
    pub cols: u16,
    pub rows: u16,
    reader: Option<std::thread::JoinHandle<()>>,
    /// Count of terminal queries the host answered (DSR/DA1), passthrough only.
    pub answered: Arc<Mutex<Vec<String>>>,
}

unsafe impl Send for Pty {}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Environment block of this process minus Claude Code's nesting markers, so a
/// nested `claude` does not refuse to start inside this agent's session.
fn env_block() -> Vec<u16> {
    let mut vars: Vec<(String, String)> = std::env::vars()
        .filter(|(k, _)| {
            let k = k.to_ascii_uppercase();
            !(k == "CLAUDECODE"
                || k == "CLAUDE_PID"
                || k.starts_with("CLAUDE_CODE_")
                || k == "CLAUDE_AGENT_SDK_VERSION"
                // The spike's host shell exports NO_COLOR=1; with it Claude
                // draws no SGR at all and screen.rs's dim test cannot work.
                || k == "NO_COLOR"
                || k == "TERM"
                || k == "COLORTERM"
                || k.starts_with("WT_"))
        })
        .collect();
    // Same as crates/pu-engine/src/engine/pty_operations.rs.
    vars.push(("TERM".into(), "xterm-256color".into()));
    vars.push(("COLORTERM".into(), "truecolor".into()));
    vars.sort_by_key(|(k, _)| k.to_ascii_uppercase());
    let mut block = Vec::new();
    for (k, v) in vars {
        block.extend(format!("{k}={v}").encode_utf16());
        block.push(0);
    }
    block.push(0);
    block
}

pub fn spawn(cmdline: &str, cwd: Option<&str>, cols: u16, rows: u16, flags: u32) -> Result<Pty, String> {
    unsafe {
        let (mut in_r, mut in_w, mut out_r, mut out_w) = (null_mut(), null_mut(), null_mut(), null_mut());
        if CreatePipe(&mut in_r, &mut in_w, null(), 0) == 0 || CreatePipe(&mut out_r, &mut out_w, null(), 0) == 0 {
            return Err(format!("CreatePipe failed: {}", GetLastError()));
        }
        let mut hpc: HPCON = 0;
        let hr = CreatePseudoConsole(COORD { X: cols as i16, Y: rows as i16 }, in_r, out_w, flags, &mut hpc);
        if hr != 0 {
            CloseHandle(in_r);
            CloseHandle(in_w);
            CloseHandle(out_r);
            CloseHandle(out_w);
            return Err(format!("CreatePseudoConsole(flags={flags:#x}) HRESULT {hr:#010x}"));
        }

        // Attribute list holding the pseudoconsole.
        let mut size = 0usize;
        InitializeProcThreadAttributeList(null_mut(), 2, 0, &mut size);
        let mut attr_buf = vec![0u8; size];
        let attrs = attr_buf.as_mut_ptr() as LPPROC_THREAD_ATTRIBUTE_LIST;
        if InitializeProcThreadAttributeList(attrs, 2, 0, &mut size) == 0 {
            return Err(format!("InitializeProcThreadAttributeList: {}", GetLastError()));
        }
        if UpdateProcThreadAttribute(
            attrs,
            0,
            PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE as usize,
            hpc as *const c_void,
            std::mem::size_of::<HPCON>(),
            null_mut(),
            null(),
        ) == 0
        {
            return Err(format!("UpdateProcThreadAttribute: {}", GetLastError()));
        }
        // Optional: PROC_THREAD_ATTRIBUTE_DESKTOP_APP_POLICY. MSIX-packaged
        // children (Store pwsh) otherwise launch their own children outside our job.
        let policy: u32 = std::env::var("SPIKE_DESKTOP_POLICY")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(2); // PROCESS_CREATION_DESKTOP_APP_BREAKAWAY_DISABLE_PROCESS_TREE
        if policy != 0
            && UpdateProcThreadAttribute(
                attrs,
                0,
                PROC_THREAD_ATTRIBUTE_DESKTOP_APP_POLICY as usize,
                &policy as *const u32 as *const c_void,
                std::mem::size_of::<u32>(),
                null_mut(),
                null(),
            ) == 0
        {
            return Err(format!("UpdateProcThreadAttribute(desktop policy): {}", GetLastError()));
        }

        let mut si: STARTUPINFOEXW = std::mem::zeroed();
        si.StartupInfo.cb = std::mem::size_of::<STARTUPINFOEXW>() as u32;
        // Without this, a parent whose stdio is redirected (pipes) leaks those
        // handles to the child instead of the pseudoconsole's.
        si.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
        si.StartupInfo.hStdInput = null_mut();
        si.StartupInfo.hStdOutput = null_mut();
        si.StartupInfo.hStdError = null_mut();
        si.lpAttributeList = attrs;

        let mut cmd = wide(cmdline);
        let env = env_block();
        let cwd_w = cwd.map(wide);
        let mut pi: PROCESS_INFORMATION = std::mem::zeroed();
        let ok = CreateProcessW(
            null(),
            cmd.as_mut_ptr(),
            null(),
            null(),
            0,
            EXTENDED_STARTUPINFO_PRESENT
                | CREATE_SUSPENDED
                | CREATE_UNICODE_ENVIRONMENT
                // Leave an inherited job (e.g. the host shell's silent-breakaway
                // job) so our job is not nested under it; see results/tree.txt.
                | if std::env::var_os("SPIKE_NO_BREAKAWAY").is_some() { 0 } else { CREATE_BREAKAWAY_FROM_JOB },
            env.as_ptr() as *const c_void,
            cwd_w.as_ref().map_or(null(), |w| w.as_ptr()),
            &si.StartupInfo,
            &mut pi,
        );
        DeleteProcThreadAttributeList(attrs);
        if ok == 0 {
            let e = GetLastError();
            ClosePseudoConsole(hpc);
            return Err(format!("CreateProcessW({cmdline}) failed: {e}"));
        }
        // ConPTY holds its own duplicates.
        CloseHandle(in_r);
        CloseHandle(out_w);

        let job = CreateJobObjectW(null(), null());
        let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        if SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            &info as *const _ as *const c_void,
            std::mem::size_of_val(&info) as u32,
        ) == 0
        {
            return Err(format!("SetInformationJobObject: {}", GetLastError()));
        }
        if AssignProcessToJobObject(job, pi.hProcess) == 0 {
            let e = GetLastError();
            TerminateProcess(pi.hProcess, 1);
            return Err(format!("AssignProcessToJobObject: {e}"));
        }
        ResumeThread(pi.hThread);
        CloseHandle(pi.hThread);

        let output = Arc::new(Mutex::new(Vec::new()));
        let answered = Arc::new(Mutex::new(Vec::new()));
        let mut out_file = File::from_raw_handle(out_r as _);
        let input = File::from_raw_handle(in_w as _);
        let passthrough = flags & PSEUDOCONSOLE_PASSTHROUGH_MODE != 0;
        let reply = if passthrough { Some(input.try_clone().unwrap()) } else { None };
        let (o, a) = (output.clone(), answered.clone());
        let reader = std::thread::spawn(move || {
            let mut reply = reply;
            let mut parser = vt100::Parser::new(rows, cols, 0);
            let mut buf = [0u8; 16384];
            loop {
                match out_file.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        let chunk = &buf[..n];
                        o.lock().unwrap().extend_from_slice(chunk);
                        parser.process(chunk);
                        // In passthrough the host is the terminal and must answer queries.
                        if let Some(w) = reply.as_mut() {
                            if contains(chunk, b"\x1b[6n") {
                                let (r, c) = parser.screen().cursor_position();
                                let _ = w.write_all(format!("\x1b[{};{}R", r + 1, c + 1).as_bytes());
                                a.lock().unwrap().push("DSR 6n".into());
                            }
                            if contains(chunk, b"\x1b[c") || contains(chunk, b"\x1b[0c") {
                                let _ = w.write_all(b"\x1b[?61;1c");
                                a.lock().unwrap().push("DA1".into());
                            }
                        }
                    }
                }
            }
        });

        Ok(Pty {
            hpc: Some(hpc),
            input,
            output,
            process: pi.hProcess,
            pid: pi.dwProcessId,
            job,
            cols,
            rows,
            reader: Some(reader),
            answered,
        })
    }
}

pub fn contains(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

impl Pty {
    pub fn write(&mut self, bytes: &[u8]) {
        let _ = self.input.write_all(bytes);
        let _ = self.input.flush();
    }

    pub fn snapshot(&self) -> Vec<u8> {
        self.output.lock().unwrap().clone()
    }

    pub fn len(&self) -> usize {
        self.output.lock().unwrap().len()
    }

    /// Waits until output is quiet for `quiet` (after at least `min`), capped at `max`.
    pub fn settle(&self, min: Duration, quiet: Duration, max: Duration) {
        let start = Instant::now();
        let mut last_len = self.len();
        let mut last_change = Instant::now();
        while start.elapsed() < max {
            std::thread::sleep(Duration::from_millis(50));
            let n = self.len();
            if n != last_len {
                last_len = n;
                last_change = Instant::now();
            }
            if start.elapsed() >= min && last_change.elapsed() >= quiet {
                break;
            }
        }
    }

    /// Polls `pred` on the parsed screen until it holds or `max` passes.
    pub fn wait_screen(&self, max: Duration, pred: impl Fn(&vt100::Screen) -> bool) -> bool {
        let start = Instant::now();
        while start.elapsed() < max {
            if pred(self.parse().screen()) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        false
    }

    pub fn parse(&self) -> vt100::Parser {
        let mut p = vt100::Parser::new(self.rows, self.cols, 0);
        p.process(&self.snapshot());
        p
    }

    pub fn resize(&mut self, cols: u16, rows: u16) -> i32 {
        self.cols = cols;
        self.rows = rows;
        unsafe { ResizePseudoConsole(self.hpc.unwrap(), COORD { X: cols as i16, Y: rows as i16 }) }
    }

    pub fn wait_exit(&self, timeout: Duration) -> Option<u32> {
        unsafe {
            if WaitForSingleObject(self.process, timeout.as_millis() as u32) == WAIT_OBJECT_0 {
                let mut code = 0u32;
                GetExitCodeProcess(self.process, &mut code);
                Some(code)
            } else {
                None
            }
        }
    }

    pub fn job_active(&self) -> u32 {
        unsafe {
            let mut info: JOBOBJECT_BASIC_ACCOUNTING_INFORMATION = std::mem::zeroed();
            QueryInformationJobObject(
                self.job,
                JobObjectBasicAccountingInformation,
                &mut info as *mut _ as *mut c_void,
                std::mem::size_of_val(&info) as u32,
                null_mut(),
            );
            info.ActiveProcesses
        }
    }

    pub fn terminate_job(&self) -> bool {
        unsafe { TerminateJobObject(self.job, 1) != 0 }
    }

    /// ClosePseudoConsole on a helper thread so a blocking close is observable.
    pub fn close_pc(&mut self) -> Duration {
        let start = Instant::now();
        if let Some(h) = self.hpc.take() {
            unsafe { ClosePseudoConsole(h) };
        }
        start.elapsed()
    }

    pub fn finish(mut self) {
        self.terminate_job();
        self.close_pc();
        if let Some(r) = self.reader.take() {
            let _ = r.join();
        }
        unsafe {
            CloseHandle(self.process);
            CloseHandle(self.job);
        }
    }
}
