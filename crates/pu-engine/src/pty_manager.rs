use std::collections::{HashMap, VecDeque};
use std::ffi::CString;
use std::os::fd::{AsRawFd, OwnedFd};
use std::sync::Arc;
use std::time::Duration;

use nix::pty::openpty;
use nix::sys::signal::{self, Signal};
use nix::sys::wait::{WaitPidFlag, WaitStatus, waitpid};
use nix::unistd::{self, ForkResult, Pid};
use tokio::sync::watch;

use crate::output_buffer::OutputBuffer;

/// Walk the process tree rooted at `root_pid` and return all descendant PIDs
/// (excluding `root_pid` itself).
///
/// `killpg` only reaches processes whose PGID matches the session leader — any
/// process that called `setsid()` or `setpgid()` (e.g. an npm dev server
/// launched with `detached: true`) creates a new process group and escapes it.
/// This function traverses by PPID instead, so those escaped processes are
/// still found and can be killed by PID directly.
async fn collect_process_descendants(root_pid: i32) -> Vec<i32> {
    // `ps -ax -o pid,ppid` lists every process on the system with its parent PID.
    let output = tokio::process::Command::new("ps")
        .args(["-ax", "-o", "pid,ppid"])
        .output()
        .await;
    let Ok(output) = output else {
        return vec![];
    };
    let Ok(stdout) = std::str::from_utf8(&output.stdout) else {
        return vec![];
    };

    // Build parent → children map (skip the header line).
    let mut children: HashMap<i32, Vec<i32>> = HashMap::new();
    for line in stdout.lines().skip(1) {
        let mut parts = line.split_whitespace();
        let pid = parts.next().and_then(|p| p.parse::<i32>().ok());
        let ppid = parts.next().and_then(|p| p.parse::<i32>().ok());
        if let (Some(pid), Some(ppid)) = (pid, ppid) {
            if pid > 0 {
                children.entry(ppid).or_default().push(pid);
            }
        }
    }

    // BFS from root_pid, collecting all descendants.
    let mut result = Vec::new();
    let mut queue = VecDeque::new();
    queue.push_back(root_pid);
    while let Some(pid) = queue.pop_front() {
        if let Some(kids) = children.get(&pid) {
            for &kid in kids {
                result.push(kid);
                queue.push_back(kid);
            }
        }
    }
    result
}

/// Fallback fd upper bound when sysconf(_SC_OPEN_MAX) fails or overflows i32.
const FD_CLOSE_UPPER_BOUND: i32 = 10240;

/// Chunked-submit tuning: bytes per write (small enough to avoid TUI paste-mode).
const CHUNK_SIZE: usize = 8;
/// Chunked-submit tuning: delay between chunks to simulate typing (ms).
const CHUNK_DELAY_MS: u64 = 6;
/// Chunked-submit tuning: delay before sending Enter so the input widget can
/// process all buffered bytes (ms).
const PRE_SUBMIT_DELAY_MS: u64 = 180;

pub struct SpawnConfig {
    pub command: String,
    pub args: Vec<String>,
    pub cwd: String,
    pub env: Vec<(String, String)>,
    pub env_remove: Vec<String>,
    pub cols: u16,
    pub rows: u16,
}

pub struct AgentHandle {
    pub pid: u32,
    pub output_buffer: Arc<OutputBuffer>,
    pub exit_rx: watch::Receiver<Option<i32>>,
    master_fd: Arc<OwnedFd>,
}

impl AgentHandle {
    /// Clone the master fd Arc for use outside the session lock.
    pub fn master_fd(&self) -> Arc<OwnedFd> {
        self.master_fd.clone()
    }
}

pub struct ProcessState {
    pub exit_code: Option<i32>,
}

pub struct NativePtyHost;

impl Default for NativePtyHost {
    fn default() -> Self {
        Self::new()
    }
}

impl NativePtyHost {
    pub fn new() -> Self {
        Self
    }

    pub async fn spawn(&self, config: SpawnConfig) -> Result<AgentHandle, std::io::Error> {
        let pty = openpty(None, None).map_err(std::io::Error::other)?;

        // Set initial window size
        let winsize = nix::pty::Winsize {
            ws_row: config.rows,
            ws_col: config.cols,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        let ret = unsafe { libc::ioctl(pty.master.as_raw_fd(), libc::TIOCSWINSZ, &winsize) };
        if ret < 0 {
            return Err(std::io::Error::last_os_error());
        }

        // Pre-allocate all CStrings BEFORE fork — child must not allocate
        // (another tokio worker thread may hold the allocator lock)
        let c_cmd = CString::new(config.command.as_str())
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;
        let mut c_args: Vec<CString> = vec![c_cmd.clone()];
        for arg in &config.args {
            c_args.push(
                CString::new(arg.as_str())
                    .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?,
            );
        }
        let c_cwd = CString::new(config.cwd.as_str())
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;
        let c_env: Vec<(CString, CString)> = config
            .env
            .iter()
            .map(|(k, v)| {
                Ok((
                    CString::new(k.as_str())
                        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?,
                    CString::new(v.as_str())
                        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?,
                ))
            })
            .collect::<Result<Vec<_>, std::io::Error>>()?;
        let c_env_remove: Vec<CString> = config
            .env_remove
            .iter()
            .map(|k| {
                CString::new(k.as_str())
                    .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))
            })
            .collect::<Result<Vec<_>, std::io::Error>>()?;

        let (exit_tx, exit_rx) = watch::channel(None);

        // Fork — safe because:
        // 1. All allocations (CStrings, env) are done above, before fork.
        // 2. Child only calls async-signal-safe libc functions before execvp.
        // 3. Child calls _exit(127) on exec failure (no unwinding).
        // posix_spawn cannot be used here because it doesn't support setsid,
        // TIOCSCTTY, or closing all fds > 2.
        match unsafe { unistd::fork() } {
            Ok(ForkResult::Child) => {
                // Child: only async-signal-safe operations from here.
                // No allocations, no drop of heap types, no Rust std::env calls.
                let slave_fd = pty.slave.as_raw_fd();

                unsafe {
                    // New session + controlling terminal
                    if libc::setsid() < 0 {
                        libc::_exit(1);
                    }
                    if libc::ioctl(slave_fd, libc::TIOCSCTTY as _, 0) < 0 {
                        libc::_exit(1);
                    }

                    // Dup slave fd to stdin/stdout/stderr
                    if libc::dup2(slave_fd, 0) < 0
                        || libc::dup2(slave_fd, 1) < 0
                        || libc::dup2(slave_fd, 2) < 0
                    {
                        libc::_exit(126);
                    }

                    // Close ALL fds >= 3 — prevents leaking master fd, epoll fds,
                    // other PTY fds, and tokio internals into the child.
                    let max_fd = libc::sysconf(libc::_SC_OPEN_MAX);
                    let upper = if max_fd > 3 && max_fd <= i32::MAX as libc::c_long {
                        core::cmp::min(max_fd as i32, FD_CLOSE_UPPER_BOUND)
                    } else {
                        FD_CLOSE_UPPER_BOUND
                    };
                    for fd in 3..upper {
                        libc::close(fd);
                    }

                    // Set cwd
                    if libc::chdir(c_cwd.as_ptr()) < 0 {
                        libc::_exit(1);
                    }

                    // Remove env vars (e.g. CLAUDECODE to avoid nested-session detection)
                    for k in &c_env_remove {
                        libc::unsetenv(k.as_ptr());
                    }

                    // Set env
                    for (k, v) in &c_env {
                        libc::setenv(k.as_ptr(), v.as_ptr(), 1);
                    }
                }

                // Exec (execvp is async-signal-safe)
                unistd::execvp(&c_cmd, &c_args).ok();
                unsafe { libc::_exit(127) };
            }
            Ok(ForkResult::Parent { child }) => {
                drop(pty.slave);
                let pid = child.as_raw() as u32;
                let buffer = Arc::new(OutputBuffer::new());
                let master_fd = Arc::new(pty.master);

                // Spawn reader task: master fd → output buffer
                // Clone Arc to keep OwnedFd alive for the task's lifetime
                let read_buf = buffer.clone();
                let fd_holder = master_fd.clone();
                tokio::task::spawn_blocking(move || {
                    let read_fd = fd_holder.as_raw_fd();
                    let mut tmp = [0u8; 4096];
                    loop {
                        let n =
                            unsafe { libc::read(read_fd, tmp.as_mut_ptr() as *mut _, tmp.len()) };
                        if n > 0 {
                            read_buf.write(&tmp[..n as usize]);
                        } else if n == 0 {
                            break; // EOF
                        } else {
                            let err = std::io::Error::last_os_error();
                            if err.kind() == std::io::ErrorKind::Interrupted {
                                continue; // EINTR — retry
                            }
                            break; // Real error
                        }
                    }
                });

                // Spawn wait task: waitpid → exit channel
                let child_pid = child;
                tokio::spawn(async move {
                    let status = tokio::task::spawn_blocking(move || {
                        loop {
                            match waitpid(child_pid, Some(WaitPidFlag::WUNTRACED)) {
                                Ok(WaitStatus::Exited(_, code)) => return Some(code),
                                Ok(WaitStatus::Signaled(_, sig, _)) => {
                                    return Some(128 + sig as i32);
                                }
                                Ok(_) => continue,
                                Err(_) => return None,
                            }
                        }
                    })
                    .await
                    .ok()
                    .flatten();
                    exit_tx.send(status).ok();
                });

                Ok(AgentHandle {
                    pid,
                    output_buffer: buffer,
                    master_fd,
                    exit_rx,
                })
            }
            Err(e) => Err(std::io::Error::other(e)),
        }
    }

    pub async fn check(&self, handle: &AgentHandle) -> Result<ProcessState, std::io::Error> {
        let exit_code = *handle.exit_rx.borrow();
        Ok(ProcessState { exit_code })
    }

    pub async fn kill(
        &self,
        handle: &AgentHandle,
        grace_period: Duration,
    ) -> Result<ProcessState, std::io::Error> {
        let raw_pid: i32 = handle.pid.try_into().map_err(|_| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "PID out of i32 range")
        })?;
        let pid = Pid::from_raw(raw_pid);

        // Snapshot the full descendant tree before signalling. Any process that
        // called setsid()/setpgid() (e.g. an npm dev server) has a different PGID
        // and would escape killpg — we'll kill those by PID directly below.
        let descendants = collect_process_descendants(raw_pid).await;

        // Send SIGTERM to the entire process group (graceful shutdown).
        // The child setsid()'d at spawn, so its PID is also its PGID. Targeting the
        // group reaps grandchildren (e.g. vitest worker forks under a user shell)
        // that would otherwise be reparented to launchd and leak.
        signal::killpg(pid, Signal::SIGTERM).ok();
        // SIGTERM any escaped descendants (separate process groups).
        for &desc in &descendants {
            unsafe {
                libc::kill(desc, libc::SIGTERM);
            }
        }

        // Poll for exit
        let deadline = tokio::time::Instant::now() + grace_period;
        loop {
            if (*handle.exit_rx.borrow()).is_some() {
                return self.check(handle).await;
            }
            if tokio::time::Instant::now() >= deadline {
                break;
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }

        // Force kill the whole group.
        signal::killpg(pid, Signal::SIGKILL).ok();
        // Force kill escaped descendants.
        for &desc in &descendants {
            unsafe {
                libc::kill(desc, libc::SIGKILL);
            }
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
        self.check(handle).await
    }

    pub async fn write_input(
        &self,
        handle: &AgentHandle,
        data: &[u8],
    ) -> Result<(), std::io::Error> {
        self.write_to_fd(&handle.master_fd, data).await
    }

    /// Write to a PTY fd directly (for use without holding a session lock).
    pub async fn write_to_fd(
        &self,
        fd_holder: &Arc<OwnedFd>,
        data: &[u8],
    ) -> Result<(), std::io::Error> {
        let fd_holder = fd_holder.clone();
        let data = data.to_vec();
        tokio::task::spawn_blocking(move || {
            let fd = fd_holder.as_raw_fd();
            let mut offset = 0;
            while offset < data.len() {
                let n = unsafe {
                    libc::write(fd, data[offset..].as_ptr() as *const _, data.len() - offset)
                };
                if n < 0 {
                    let err = std::io::Error::last_os_error();
                    if err.kind() == std::io::ErrorKind::Interrupted {
                        continue; // EINTR — retry
                    }
                    return Err(err);
                }
                offset += n as usize;
            }
            Ok(())
        })
        .await
        .map_err(std::io::Error::other)?
    }

    /// Write data to a PTY fd in small chunks (simulating typing) then submit
    /// with Enter (`\r`).  The chunking plus a delay before Enter avoids a race
    /// where TUI applications (e.g. Claude Code) swallow the Enter keypress
    /// when text and Enter arrive in a single atomic write.
    pub async fn write_chunked_submit(
        &self,
        fd_holder: &Arc<OwnedFd>,
        data: &[u8],
    ) -> Result<(), std::io::Error> {
        // Write text in 8-byte chunks with short delays to mimic typing and
        // avoid triggering TUI paste-mode detection.
        for chunk in data.chunks(CHUNK_SIZE) {
            self.write_to_fd(fd_holder, chunk).await?;
            tokio::time::sleep(Duration::from_millis(CHUNK_DELAY_MS)).await;
        }
        // Give the input widget time to process buffered bytes before submit.
        tokio::time::sleep(Duration::from_millis(PRE_SUBMIT_DELAY_MS)).await;
        self.write_to_fd(fd_holder, b"\r").await
    }

    pub async fn resize(
        &self,
        handle: &AgentHandle,
        cols: u16,
        rows: u16,
    ) -> Result<(), std::io::Error> {
        self.resize_fd(&handle.master_fd, cols, rows).await
    }

    /// Resize a PTY fd directly (for use without holding a session lock).
    pub async fn resize_fd(
        &self,
        fd_holder: &Arc<OwnedFd>,
        cols: u16,
        rows: u16,
    ) -> Result<(), std::io::Error> {
        let fd_holder = fd_holder.clone();
        let winsize = nix::pty::Winsize {
            ws_row: rows,
            ws_col: cols,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        tokio::task::spawn_blocking(move || {
            let fd = fd_holder.as_raw_fd();
            let ret = unsafe { libc::ioctl(fd, libc::TIOCSWINSZ, &winsize) };
            if ret < 0 {
                Err(std::io::Error::last_os_error())
            } else {
                Ok(())
            }
        })
        .await
        .map_err(std::io::Error::other)?
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test(flavor = "current_thread")]
    async fn given_spawn_should_return_handle_with_pid() {
        // given
        let host = NativePtyHost::new();

        // when
        let handle = host
            .spawn(SpawnConfig {
                command: "/bin/echo".into(),
                args: vec!["hello".into()],
                cwd: "/tmp".into(),
                env: vec![],
                env_remove: vec![],
                cols: 80,
                rows: 24,
            })
            .await
            .unwrap();

        // then
        assert!(handle.pid > 0);
        // Wait for process to complete
        tokio::time::sleep(Duration::from_millis(200)).await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn given_spawned_echo_should_capture_output() {
        let host = NativePtyHost::new();
        let handle = host
            .spawn(SpawnConfig {
                command: "/bin/echo".into(),
                args: vec!["hello_pty_test".into()],
                cwd: "/tmp".into(),
                env: vec![],
                env_remove: vec![],
                cols: 80,
                rows: 24,
            })
            .await
            .unwrap();

        // Wait for output to be captured
        tokio::time::sleep(Duration::from_millis(500)).await;

        let output = handle.output_buffer.read_all();
        let text = String::from_utf8_lossy(&output);
        assert!(
            text.contains("hello_pty_test"),
            "expected output to contain 'hello_pty_test', got: {text}"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn given_spawned_process_should_detect_exit() {
        let host = NativePtyHost::new();
        let handle = host
            .spawn(SpawnConfig {
                command: "/bin/echo".into(),
                args: vec!["done".into()],
                cwd: "/tmp".into(),
                env: vec![],
                env_remove: vec![],
                cols: 80,
                rows: 24,
            })
            .await
            .unwrap();

        // Wait for exit
        tokio::time::sleep(Duration::from_millis(500)).await;

        let state = host.check(&handle).await.unwrap();
        assert!(
            state.exit_code.is_some(),
            "expected exit code, process still running"
        );
        assert_eq!(state.exit_code.unwrap(), 0);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn given_failing_command_should_capture_nonzero_exit() {
        let host = NativePtyHost::new();
        let handle = host
            .spawn(SpawnConfig {
                command: "/bin/sh".into(),
                args: vec!["-c".into(), "exit 42".into()],
                cwd: "/tmp".into(),
                env: vec![],
                env_remove: vec![],
                cols: 80,
                rows: 24,
            })
            .await
            .unwrap();

        tokio::time::sleep(Duration::from_millis(500)).await;

        let state = host.check(&handle).await.unwrap();
        assert_eq!(state.exit_code, Some(42));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn given_running_process_should_kill_with_signal() {
        let host = NativePtyHost::new();
        let handle = host
            .spawn(SpawnConfig {
                command: "/bin/sleep".into(),
                args: vec!["60".into()],
                cwd: "/tmp".into(),
                env: vec![],
                env_remove: vec![],
                cols: 80,
                rows: 24,
            })
            .await
            .unwrap();

        // Verify it's running
        tokio::time::sleep(Duration::from_millis(100)).await;
        let state = host.check(&handle).await.unwrap();
        assert!(state.exit_code.is_none(), "should still be running");

        // Kill it
        let exit = host.kill(&handle, Duration::from_secs(2)).await.unwrap();
        assert!(exit.exit_code.is_some(), "should have exited after kill");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn given_spawned_process_should_write_input() {
        let host = NativePtyHost::new();
        let handle = host
            .spawn(SpawnConfig {
                command: "/bin/cat".into(),
                args: vec![],
                cwd: "/tmp".into(),
                env: vec![],
                env_remove: vec![],
                cols: 80,
                rows: 24,
            })
            .await
            .unwrap();

        // Write input to cat, which should echo it back
        host.write_input(&handle, b"test_input\n").await.unwrap();
        tokio::time::sleep(Duration::from_millis(300)).await;

        let output = handle.output_buffer.read_all();
        let text = String::from_utf8_lossy(&output);
        assert!(
            text.contains("test_input"),
            "expected echoed input, got: {text}"
        );

        // Clean up
        host.kill(&handle, Duration::from_secs(1)).await.ok();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn given_spawned_process_should_resize() {
        let host = NativePtyHost::new();
        let handle = host
            .spawn(SpawnConfig {
                command: "/bin/sleep".into(),
                args: vec!["5".into()],
                cwd: "/tmp".into(),
                env: vec![],
                env_remove: vec![],
                cols: 80,
                rows: 24,
            })
            .await
            .unwrap();

        // Resize should not error
        let result = host.resize(&handle, 120, 40).await;
        assert!(result.is_ok());

        host.kill(&handle, Duration::from_secs(1)).await.ok();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn given_chunked_submit_should_deliver_text_and_enter() {
        // given — a cat process that echoes input
        let host = NativePtyHost::new();
        let handle = host
            .spawn(SpawnConfig {
                command: "/bin/cat".into(),
                args: vec![],
                cwd: "/tmp".into(),
                env: vec![],
                env_remove: vec![],
                cols: 80,
                rows: 24,
            })
            .await
            .unwrap();

        // when — send text via chunked submit (simulates `pu send agent "text"`)
        host.write_chunked_submit(&handle.master_fd, b"hello_chunked")
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(500)).await;

        // then — output should contain the text (echoed by cat)
        let output = handle.output_buffer.read_all();
        let text = String::from_utf8_lossy(&output);
        assert!(
            text.contains("hello_chunked"),
            "expected echoed text, got: {text}"
        );
        // The Enter (\r) should also have been delivered (cat echoes it as \r\n)
        assert!(
            text.contains('\r') || text.contains('\n'),
            "expected newline from Enter submission, got: {text}"
        );

        host.kill(&handle, Duration::from_secs(1)).await.ok();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn given_chunked_submit_with_empty_data_should_send_only_enter() {
        // given — a cat process
        let host = NativePtyHost::new();
        let handle = host
            .spawn(SpawnConfig {
                command: "/bin/cat".into(),
                args: vec![],
                cwd: "/tmp".into(),
                env: vec![],
                env_remove: vec![],
                cols: 80,
                rows: 24,
            })
            .await
            .unwrap();

        // when — chunked submit with empty data (just Enter)
        host.write_chunked_submit(&handle.master_fd, b"")
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(500)).await;

        // then — should not error (Enter is still sent)
        let output = handle.output_buffer.read_all();
        let text = String::from_utf8_lossy(&output);
        // cat echoes the \r as a newline
        assert!(
            text.contains('\r') || text.contains('\n'),
            "expected newline from Enter, got bytes: {output:?}"
        );

        host.kill(&handle, Duration::from_secs(1)).await.ok();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn given_killed_agent_should_reap_grandchildren() {
        let host = NativePtyHost::new();
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let path = tmp.path().to_string_lossy().into_owned();

        // Shell that backgrounds a long sleep, records its PID, then waits.
        // The sleep is a grandchild of the kill target — under the old single-PID
        // kill it would orphan to launchd; under killpg it should be reaped.
        let handle = host
            .spawn(SpawnConfig {
                command: "/bin/sh".into(),
                args: vec![
                    "-c".into(),
                    format!("sleep 300 & echo $! > {path}; wait"),
                ],
                cwd: "/tmp".into(),
                env: vec![],
                env_remove: vec![],
                cols: 80,
                rows: 24,
            })
            .await
            .unwrap();

        let mut grandchild_pid: i32 = 0;
        for _ in 0..50 {
            tokio::time::sleep(Duration::from_millis(50)).await;
            if let Ok(s) = std::fs::read_to_string(&path)
                && let Ok(pid) = s.trim().parse::<i32>()
                && pid > 0
            {
                grandchild_pid = pid;
                break;
            }
        }
        assert!(grandchild_pid > 0, "grandchild PID was not recorded");

        let alive_before = unsafe { libc::kill(grandchild_pid, 0) };
        assert_eq!(alive_before, 0, "grandchild should be alive before kill");

        host.kill(&handle, Duration::from_secs(2)).await.unwrap();
        tokio::time::sleep(Duration::from_millis(200)).await;

        let alive_after = unsafe { libc::kill(grandchild_pid, 0) };
        assert_ne!(
            alive_after, 0,
            "grandchild PID {grandchild_pid} should be dead after group kill"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn given_killed_agent_should_reap_setsid_escaped_descendants() {
        // Simulates an npm dev server: a descendant that called setsid() and is
        // therefore in its own process group, escaping killpg. The tree-walk kill
        // should still find and terminate it by PID.
        let host = NativePtyHost::new();
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let path = tmp.path().to_string_lossy().into_owned();

        // Shell that backgrounds a perl process which immediately calls setsid()
        // (escaping the agent's process group), records its PID, then sleeps.
        // The shell stays alive via `wait` so perl remains its child in the ppid
        // tree long enough for collect_process_descendants to find it.
        let handle = host
            .spawn(SpawnConfig {
                command: "/bin/sh".into(),
                args: vec![
                    "-c".into(),
                    format!(
                        r#"perl -MPOSIX -e 'POSIX::setsid(); open my $f, q(>), q({path}); print $f $$; close $f; sleep 300' & wait"#,
                        path = path
                    ),
                ],
                cwd: "/tmp".into(),
                env: vec![],
                env_remove: vec![],
                cols: 80,
                rows: 24,
            })
            .await
            .unwrap();

        let mut escaped_pid: i32 = 0;
        for _ in 0..50 {
            tokio::time::sleep(Duration::from_millis(50)).await;
            if let Ok(s) = std::fs::read_to_string(&path)
                && let Ok(pid) = s.trim().parse::<i32>()
                && pid > 0
            {
                escaped_pid = pid;
                break;
            }
        }
        assert!(escaped_pid > 0, "escaped child PID was not recorded");

        // Confirm it escaped to its own process group.
        let escaped_pgid = unsafe { libc::getpgid(escaped_pid) };
        assert_ne!(
            escaped_pgid,
            handle.pid as libc::pid_t,
            "escaped child should be in a different process group than the agent"
        );

        let alive_before = unsafe { libc::kill(escaped_pid, 0) };
        assert_eq!(alive_before, 0, "escaped child should be alive before kill");

        host.kill(&handle, Duration::from_secs(2)).await.unwrap();
        tokio::time::sleep(Duration::from_millis(200)).await;

        let alive_after = unsafe { libc::kill(escaped_pid, 0) };
        assert_ne!(
            alive_after, 0,
            "escaped child PID {escaped_pid} should be dead after tree-walk kill"
        );
    }
}
