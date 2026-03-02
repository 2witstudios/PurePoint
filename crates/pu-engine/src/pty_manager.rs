use std::ffi::CString;
use std::os::fd::{AsRawFd, OwnedFd};
use std::sync::Arc;
use std::time::Duration;

use nix::pty::openpty;
use nix::sys::signal::{self, Signal};
use nix::sys::wait::{waitpid, WaitPidFlag, WaitStatus};
use nix::unistd::{self, ForkResult, Pid};
use tokio::sync::watch;

use crate::output_buffer::OutputBuffer;

pub struct SpawnConfig {
    pub command: String,
    pub args: Vec<String>,
    pub cwd: String,
    pub env: Vec<(String, String)>,
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
        let ret = unsafe {
            libc::ioctl(pty.master.as_raw_fd(), libc::TIOCSWINSZ, &winsize)
        };
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
                    libc::setsid();
                    libc::ioctl(slave_fd, libc::TIOCSCTTY as _, 0);

                    // Dup slave fd to stdin/stdout/stderr
                    if libc::dup2(slave_fd, 0) < 0
                        || libc::dup2(slave_fd, 1) < 0
                        || libc::dup2(slave_fd, 2) < 0
                    {
                        libc::_exit(126);
                    }

                    // Close ALL fds >= 3 — prevents leaking master fd, epoll fds,
                    // other PTY fds, and tokio internals into the child.
                    // Cap at a sane max to avoid iterating millions on macOS
                    // where _SC_OPEN_MAX can be huge.
                    let max_fd = libc::sysconf(libc::_SC_OPEN_MAX) as i32;
                    let upper = if max_fd > 0 && max_fd < 8192 { max_fd } else { 1024 };
                    for fd in 3..upper {
                        libc::close(fd);
                    }

                    // Set cwd
                    libc::chdir(c_cwd.as_ptr());

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
                        let n = unsafe {
                            libc::read(read_fd, tmp.as_mut_ptr() as *mut _, tmp.len())
                        };
                        if n > 0 {
                            read_buf.write(&tmp[..n as usize]);
                        } else {
                            break; // EOF or error
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
                                Ok(WaitStatus::Signaled(_, sig, _)) => return Some(128 + sig as i32),
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

    pub async fn kill(&self, handle: &AgentHandle, grace_period: Duration) -> Result<ProcessState, std::io::Error> {
        let raw_pid: i32 = handle.pid.try_into().map_err(|_| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "PID out of i32 range")
        })?;
        let pid = Pid::from_raw(raw_pid);

        // Send SIGTERM first (graceful shutdown)
        signal::kill(pid, Signal::SIGTERM).ok();

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

        // Force kill
        signal::kill(pid, Signal::SIGKILL).ok();
        tokio::time::sleep(Duration::from_millis(100)).await;
        self.check(handle).await
    }

    pub async fn write_input(&self, handle: &AgentHandle, data: &[u8]) -> Result<(), std::io::Error> {
        self.write_to_fd(&handle.master_fd, data).await
    }

    /// Write to a PTY fd directly (for use without holding a session lock).
    pub async fn write_to_fd(&self, fd_holder: &Arc<OwnedFd>, data: &[u8]) -> Result<(), std::io::Error> {
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
                    return Err(std::io::Error::last_os_error());
                }
                offset += n as usize;
            }
            Ok(())
        })
        .await
        .map_err(std::io::Error::other)?
    }

    pub async fn resize(&self, handle: &AgentHandle, cols: u16, rows: u16) -> Result<(), std::io::Error> {
        self.resize_fd(&handle.master_fd, cols, rows).await
    }

    /// Resize a PTY fd directly (for use without holding a session lock).
    pub async fn resize_fd(&self, fd_holder: &Arc<OwnedFd>, cols: u16, rows: u16) -> Result<(), std::io::Error> {
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
                cols: 80,
                rows: 24,
            })
            .await
            .unwrap();

        // Wait for exit
        tokio::time::sleep(Duration::from_millis(500)).await;

        let state = host.check(&handle).await.unwrap();
        assert!(state.exit_code.is_some(), "expected exit code, process still running");
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
}
