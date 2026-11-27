//! Local git and gh processes: `LocalProjectHost.run` from `Services/ProjectHost.swift` and
//! the gh lookup from `Services/GitService.swift`.
//!
//! Children are created with `CREATE_NO_WINDOW` (REQ-WINAPP-024), so no console flashes.
//! Both pipes drain on their own threads while the caller waits, an output limit stops
//! capture (and the process) once exceeded, and a deadline or a cancelled token ends the
//! whole process tree.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

/// What a git or gh run reported.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CommandResult {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
    pub output_exceeded_limit: bool,
}

impl CommandResult {
    pub fn failure(stderr: impl Into<String>, exit_code: i32) -> Self {
        Self {
            stderr: stderr.into(),
            exit_code,
            ..Self::default()
        }
    }

    pub fn success(&self) -> bool {
        self.exit_code == 0
    }
}

/// Cooperative cancellation, standing in for Swift task cancellation. Clones share one flag.
#[derive(Debug, Clone, Default)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

/// A git or gh invocation. Locally `executable_path` is run; remotely the daemon runs `program`.
#[derive(Debug, Clone, Default)]
pub struct HostCommand {
    pub program: String,
    pub executable_path: String,
    pub args: Vec<String>,
    pub cwd: String,
    pub stdin: Option<Vec<u8>>,
    pub timeout: Option<Duration>,
    pub output_limit: Option<usize>,
    /// Stdout goes to this file instead of `CommandResult::stdout`.
    pub output_file: Option<PathBuf>,
    pub cancel: Option<CancelToken>,
}

/// `CREATE_NO_WINDOW` from `winbase.h`.
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// How often the waiting thread looks at the deadline, the limit and the cancel token.
const POLL: Duration = Duration::from_millis(20);

/// After a forced stop, how long to wait for the pipe readers before giving up on them
/// (an orphaned grandchild could otherwise hold a pipe open).
const DRAIN_GRACE: Duration = Duration::from_secs(2);

/// Hides the console window of a helper process on Windows; a no-op elsewhere.
pub fn hide_console(command: &mut Command) -> &mut Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command
}

/// The git executable on PATH, resolved once. Falls back to the bare name, which lets the
/// spawn report the missing binary.
pub fn git_path() -> &'static str {
    static GIT: OnceLock<String> = OnceLock::new();
    GIT.get_or_init(|| {
        which::which("git")
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_else(|_| "git".into())
    })
}

/// gh on PATH, then its default install locations (`locateGh` on the Mac).
pub fn locate_gh() -> Option<String> {
    if let Ok(path) = which::which("gh") {
        return Some(path.to_string_lossy().into_owned());
    }
    gh_candidates()
        .into_iter()
        .find(|path| path.is_file())
        .map(|path| path.to_string_lossy().into_owned())
}

fn gh_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    #[cfg(windows)]
    {
        for variable in ["ProgramFiles", "ProgramW6432", "ProgramFiles(x86)"] {
            if let Some(dir) = std::env::var_os(variable) {
                candidates.push(Path::new(&dir).join("GitHub CLI").join("gh.exe"));
            }
        }
        if let Some(dir) = std::env::var_os("LOCALAPPDATA") {
            candidates.push(
                Path::new(&dir)
                    .join("Programs")
                    .join("GitHub CLI")
                    .join("gh.exe"),
            );
        }
    }
    #[cfg(not(windows))]
    {
        candidates.push(PathBuf::from("/opt/homebrew/bin/gh"));
        candidates.push(PathBuf::from("/usr/local/bin/gh"));
    }
    candidates
}

/// Runs `command` on this machine and waits for it.
pub fn run_local(command: &HostCommand) -> CommandResult {
    let mut process = Command::new(&command.executable_path);
    process
        .args(&command.args)
        .current_dir(&command.cwd)
        .stderr(Stdio::piped());
    hide_console(&mut process);
    process.stdin(if command.stdin.is_some() {
        Stdio::piped()
    } else {
        Stdio::null()
    });
    if let Some(path) = &command.output_file {
        match std::fs::File::create(path) {
            Ok(file) => {
                process.stdout(file);
            }
            Err(error) => return CommandResult::failure(error.to_string(), -1),
        }
    } else {
        process.stdout(Stdio::piped());
    }

    let mut child = match process.spawn() {
        Ok(child) => child,
        Err(error) => return CommandResult::failure(error.to_string(), -1),
    };

    if let (Some(mut pipe), Some(input)) = (child.stdin.take(), command.stdin.clone()) {
        thread::spawn(move || {
            let _ = pipe.write_all(&input);
        });
    }

    let exceeded = Arc::new(AtomicBool::new(false));
    let (stdout_tx, stdout_rx) = mpsc::channel::<Vec<u8>>();
    let stdout_reader = child.stdout.take().map(|mut pipe| {
        let limit = command.output_limit;
        let exceeded = exceeded.clone();
        thread::spawn(move || {
            let mut data = Vec::new();
            let mut chunk = [0u8; 16_384];
            // Fixed-size reads bound capture. Keep draining after the limit so a full pipe
            // cannot keep the process from exiting.
            loop {
                let read = match pipe.read(&mut chunk) {
                    Ok(0) | Err(_) => break,
                    Ok(read) => read,
                };
                if exceeded.load(Ordering::SeqCst) {
                    continue;
                }
                match limit {
                    Some(limit) if read > limit.saturating_sub(data.len()) => {
                        exceeded.store(true, Ordering::SeqCst);
                        data = Vec::new();
                    }
                    _ => data.extend_from_slice(&chunk[..read]),
                }
            }
            let _ = stdout_tx.send(data);
        })
    });
    let (stderr_tx, stderr_rx) = mpsc::channel::<Vec<u8>>();
    let stderr_reader = child.stderr.take().map(|mut pipe| {
        thread::spawn(move || {
            let mut data = Vec::new();
            let _ = pipe.read_to_end(&mut data);
            let _ = stderr_tx.send(data);
        })
    });

    let deadline = command.timeout.map(|timeout| Instant::now() + timeout);
    let mut forced = false;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) => {}
            Err(_) => break None,
        }
        let cancelled = command
            .cancel
            .as_ref()
            .is_some_and(CancelToken::is_cancelled);
        let late = deadline.is_some_and(|deadline| Instant::now() >= deadline);
        if !forced && (cancelled || late || exceeded.load(Ordering::SeqCst)) {
            forced = true;
            kill_tree(&mut child);
        }
        thread::sleep(POLL);
    };

    let collect =
        |rx: &mpsc::Receiver<Vec<u8>>, reader: Option<thread::JoinHandle<()>>| -> Vec<u8> {
            let Some(reader) = reader else {
                return Vec::new();
            };
            let data = if forced {
                rx.recv_timeout(DRAIN_GRACE).unwrap_or_default()
            } else {
                rx.recv().unwrap_or_default()
            };
            if !forced {
                let _ = reader.join();
            }
            data
        };
    let stdout = collect(&stdout_rx, stdout_reader);
    let stderr = collect(&stderr_rx, stderr_reader);
    let output_exceeded_limit = exceeded.load(Ordering::SeqCst);
    CommandResult {
        stdout: if output_exceeded_limit {
            String::new()
        } else {
            String::from_utf8(stdout).unwrap_or_default()
        },
        stderr: String::from_utf8(stderr).unwrap_or_default(),
        exit_code: status.and_then(|status| status.code()).unwrap_or(-1),
        output_exceeded_limit,
    }
}

/// `%SystemRoot%\System32\taskkill.exe`, never a `taskkill` found on PATH or in the current
/// directory (the worktree, which an agent can write).
#[cfg(windows)]
fn taskkill_path() -> std::path::PathBuf {
    let root = std::env::var_os("SystemRoot")
        .filter(|root| std::path::Path::new(root).is_absolute())
        .unwrap_or_else(|| r"C:\Windows".into());
    std::path::Path::new(&root)
        .join("System32")
        .join("taskkill.exe")
}

/// Ends the child and everything it started. git runs aliases and hooks through a shell, so
/// stopping git.exe alone would leave its children holding the pipes.
fn kill_tree(child: &mut Child) {
    #[cfg(windows)]
    {
        let mut taskkill = Command::new(taskkill_path());
        taskkill.args(["/T", "/F", "/PID", &child.id().to_string()]);
        taskkill
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        hide_console(&mut taskkill);
        if taskkill.status().is_ok_and(|status| status.success()) {
            return;
        }
    }
    let _ = child.kill();
}

#[cfg(all(test, windows))]
mod taskkill_tests {
    #[test]
    fn taskkill_is_the_system_copy() {
        let path = super::taskkill_path();
        assert!(path.is_absolute(), "{path:?}");
        assert!(path.ends_with("System32/taskkill.exe"), "{path:?}");
        assert!(path.exists(), "{path:?}");
    }
}
