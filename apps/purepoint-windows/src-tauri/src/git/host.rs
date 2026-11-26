//! Port of `Services/ProjectHost.swift`: where a project's files live and its git runs
//! (MACH-007). Local projects use the filesystem and processes directly; remote projects go
//! through their machine's daemon (`fs_*` and `git_exec`), which confines every path to its
//! registered projects.
//!
//! The daemon side is a [`GitExec`]: one blocking request and its reply. WIN-5.3 wires the
//! real daemon client; until then [`UnavailableGitExec`] answers every request with the
//! Mac's "machine is not available" failure.
//!
//! Every method blocks; callers stay off the UI thread. Directory watching (`makeWatcher`,
//! `DaemonDirectoryWatcher`, `DaemonFsSubscription`) lives with the watchers and
//! subscriptions modules; [`SubscriptionBackoff`] is ported here for them.

use std::fmt;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use pu_core::protocol::{Request, Response};

use super::runner::{self, CommandResult, HostCommand};

/// One directory entry as a host reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostFileEntry {
    pub name: String,
    pub path: String,
    pub is_directory: bool,
    pub is_symlink: bool,
    pub size: u64,
    pub modified_ms: i64,
    /// Remote listings carry the daemon's `git check-ignore` verdict; local listings leave
    /// it to the caller.
    pub ignored: Option<bool>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HostFileStat {
    pub exists: bool,
    pub is_directory: bool,
    pub size: u64,
    pub modified_ms: i64,
}

impl HostFileStat {
    pub const MISSING: Self = Self {
        exists: false,
        is_directory: false,
        size: 0,
        modified_ms: 0,
    };
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostFileData {
    pub data: Vec<u8>,
    pub size: u64,
    pub modified_ms: i64,
    /// The file is larger than the requested maximum; `data` holds only its start.
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostError {
    PathNotAllowed(String),
    Conflict(String),
    NotFound(String),
    TooLargeToSave,
    Daemon { code: String, message: String },
    UnexpectedResponse(String),
    Io(String),
}

impl HostError {
    /// Saves travel in one request line, which the daemon caps at 1 MB.
    pub const MAX_REMOTE_SAVE_BYTES: usize = 750_000;

    pub fn from_code(code: &str, message: &str) -> Self {
        match code {
            "path_not_allowed" => Self::PathNotAllowed(message.into()),
            "conflict" => Self::Conflict(message.into()),
            "not_found" => Self::NotFound(message.into()),
            _ => Self::Daemon {
                code: code.into(),
                message: message.into(),
            },
        }
    }

    /// The failure of a host whose machine is offline or gone.
    pub fn unavailable() -> Self {
        Self::Daemon {
            code: "unavailable".into(),
            message: "This project's machine is not available.".into(),
        }
    }
}

impl fmt::Display for HostError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PathNotAllowed(message) | Self::NotFound(message) | Self::Io(message) => f.write_str(message),
            Self::Conflict(_) => {
                f.write_str("The file changed on the remote machine since it was opened. Reload it before saving.")
            }
            Self::TooLargeToSave => f.write_str("This file is too large to save on a remote machine (over 750 KB)."),
            Self::Daemon { message, .. } => f.write_str(message),
            Self::UnexpectedResponse(kind) => write!(f, "Unexpected response from the remote daemon: {kind}"),
        }
    }
}

impl std::error::Error for HostError {}

impl From<std::io::Error> for HostError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error.to_string())
    }
}

/// Where a project's files live and its git and gh run.
pub trait ProjectHost: Send + Sync {
    fn is_local(&self) -> bool;
    /// Distinguishes hosts in caches keyed by path.
    fn identity(&self) -> String;

    /// Immediate children of a directory, hidden files included.
    fn list(&self, directory: &str) -> Result<Vec<HostFileEntry>, HostError>;
    /// Every file and directory below `root`, up to `limit` entries, and whether it stopped short.
    fn list_recursive(
        &self,
        root: &str,
        limit: usize,
    ) -> Result<(Vec<HostFileEntry>, bool), HostError>;
    fn stat(&self, path: &str) -> Result<HostFileStat, HostError>;
    fn read(&self, path: &str, max_bytes: usize) -> Result<HostFileData, HostError>;
    fn write(
        &self,
        path: &str,
        data: &[u8],
        expected_modified_ms: Option<i64>,
        create_directories: bool,
    ) -> Result<HostFileStat, HostError>;
    fn delete(&self, path: &str) -> Result<(), HostError>;
    fn run(&self, command: &HostCommand) -> CommandResult;

    /// `directory` joined with a git (`/`-separated) relative path. Remote machines use `/`.
    fn join(&self, directory: &str, name: &str) -> String {
        if self.is_local() {
            Path::new(directory)
                .join(name)
                .to_string_lossy()
                .into_owned()
        } else if directory.ends_with('/') {
            format!("{directory}{name}")
        } else {
            format!("{directory}/{name}")
        }
    }

    /// The directory part of `path` (`deletingLastPathComponent`).
    fn parent(&self, path: &str) -> String {
        if self.is_local() {
            return Path::new(path)
                .parent()
                .map(|parent| parent.to_string_lossy().into_owned())
                .unwrap_or_default();
        }
        match path.trim_end_matches('/').rfind('/') {
            Some(0) => "/".into(),
            Some(index) => path[..index].into(),
            None => String::new(),
        }
    }
}

// MARK: - Local

/// This machine: the filesystem and child processes.
#[derive(Debug, Clone, Copy, Default)]
pub struct LocalProjectHost;

impl LocalProjectHost {
    pub fn milliseconds(time: Option<SystemTime>) -> i64 {
        time.and_then(|time| time.duration_since(UNIX_EPOCH).ok())
            .map(|elapsed| elapsed.as_millis() as i64)
            .unwrap_or(0)
    }

    fn entry(name: String, path: &Path) -> Option<HostFileEntry> {
        let metadata = std::fs::symlink_metadata(path).ok()?;
        let is_symlink = metadata.file_type().is_symlink();
        let is_directory = if is_symlink {
            std::fs::metadata(path).ok()?.is_dir()
        } else {
            metadata.is_dir()
        };
        Some(HostFileEntry {
            name,
            path: path.to_string_lossy().into_owned(),
            is_directory,
            is_symlink,
            size: metadata.len(),
            modified_ms: Self::milliseconds(metadata.modified().ok()),
            ignored: None,
        })
    }
}

impl ProjectHost for LocalProjectHost {
    fn is_local(&self) -> bool {
        true
    }

    fn identity(&self) -> String {
        "local".into()
    }

    fn list(&self, directory: &str) -> Result<Vec<HostFileEntry>, HostError> {
        let mut entries = Vec::new();
        for child in std::fs::read_dir(directory)?.flatten() {
            let name = child.file_name().to_string_lossy().into_owned();
            if let Some(entry) = Self::entry(name, &child.path()) {
                entries.push(entry);
            }
        }
        Ok(entries)
    }

    fn list_recursive(
        &self,
        root: &str,
        limit: usize,
    ) -> Result<(Vec<HostFileEntry>, bool), HostError> {
        let mut entries = Vec::new();
        let mut stack = vec![Path::new(root).to_path_buf()];
        while let Some(directory) = stack.pop() {
            let Ok(children) = std::fs::read_dir(&directory) else {
                continue;
            };
            for child in children.flatten() {
                if entries.len() >= limit {
                    return Ok((entries, true));
                }
                let name = child.file_name().to_string_lossy().into_owned();
                if let Some(entry) = Self::entry(name, &child.path()) {
                    if entry.is_directory && !entry.is_symlink {
                        stack.push(child.path());
                    }
                    entries.push(entry);
                }
            }
        }
        Ok((entries, false))
    }

    fn stat(&self, path: &str) -> Result<HostFileStat, HostError> {
        let Ok(metadata) = std::fs::symlink_metadata(path) else {
            return Ok(HostFileStat::MISSING);
        };
        Ok(HostFileStat {
            exists: true,
            is_directory: metadata.is_dir(),
            size: metadata.len(),
            modified_ms: Self::milliseconds(metadata.modified().ok()),
        })
    }

    fn read(&self, path: &str, max_bytes: usize) -> Result<HostFileData, HostError> {
        use std::io::Read;
        let metadata = std::fs::metadata(path)?;
        let mut data = Vec::new();
        std::fs::File::open(path)?
            .take(max_bytes as u64 + 1)
            .read_to_end(&mut data)?;
        let truncated = data.len() > max_bytes;
        data.truncate(max_bytes);
        Ok(HostFileData {
            data,
            size: metadata.len(),
            modified_ms: Self::milliseconds(metadata.modified().ok()),
            truncated,
        })
    }

    fn write(
        &self,
        path: &str,
        data: &[u8],
        expected_modified_ms: Option<i64>,
        create_directories: bool,
    ) -> Result<HostFileStat, HostError> {
        if let Some(expected) = expected_modified_ms
            && self.stat(path)?.modified_ms != expected
        {
            return Err(HostError::Conflict(path.into()));
        }
        let target = Path::new(path);
        if create_directories && let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        // Atomic replace: a temp file beside the target, renamed over it.
        let temp = target.with_file_name(format!(
            ".{}.purepoint-save",
            target
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default()
        ));
        std::fs::write(&temp, data)?;
        if let Err(error) = std::fs::rename(&temp, target) {
            let _ = std::fs::remove_file(&temp);
            return Err(error.into());
        }
        self.stat(path)
    }

    fn delete(&self, path: &str) -> Result<(), HostError> {
        let metadata = std::fs::symlink_metadata(path)?;
        if metadata.is_dir() {
            std::fs::remove_dir_all(path)?
        } else {
            std::fs::remove_file(path)?
        }
        Ok(())
    }

    fn run(&self, command: &HostCommand) -> CommandResult {
        runner::run_local(command)
    }
}

// MARK: - Remote (daemon)

/// A one-shot request to a machine's daemon: `git_exec`, plus the `fs_*` reads the review
/// needs. The daemon implementation is wired by WIN-5.3.
pub trait GitExec: Send + Sync {
    fn send(&self, request: Request, timeout: Duration) -> Result<Response, HostError>;
}

/// Stands in for the daemon client until WIN-5.3: every request fails as "not available".
#[derive(Debug, Clone, Copy, Default)]
pub struct UnavailableGitExec;

impl GitExec for UnavailableGitExec {
    fn send(&self, _request: Request, _timeout: Duration) -> Result<Response, HostError> {
        Err(HostError::unavailable())
    }
}

/// A project on another machine: files and git go through that machine's daemon.
pub struct DaemonProjectHost<E: GitExec> {
    pub client: E,
    pub machine_id: String,
}

impl<E: GitExec> DaemonProjectHost<E> {
    /// Upper bound for one listing; the daemon also enforces its own.
    pub const MAX_LIST_ENTRIES: usize = 20_000;
    /// One-shot requests time out after 30 s (REQ-WINAPP-059).
    const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

    pub fn new(client: E, machine_id: impl Into<String>) -> Self {
        Self {
            client,
            machine_id: machine_id.into(),
        }
    }

    pub fn list_request(directory: &str) -> Request {
        Request::FsList {
            path: directory.into(),
            recursive: false,
            include_hidden: true,
            max_entries: Self::MAX_LIST_ENTRIES,
            directories_only: false,
        }
    }

    pub fn list_recursive_request(root: &str, limit: usize) -> Request {
        Request::FsList {
            path: root.into(),
            recursive: true,
            include_hidden: true,
            max_entries: limit,
            directories_only: false,
        }
    }

    pub fn read_request(path: &str, max_bytes: usize) -> Request {
        Request::FsRead {
            path: path.into(),
            max_bytes: max_bytes as u64,
        }
    }

    pub fn write_request(
        path: &str,
        data: &[u8],
        expected_modified_ms: Option<i64>,
        create_directories: bool,
    ) -> Request {
        Request::FsWrite {
            path: path.into(),
            data_base64: BASE64.encode(data),
            expected_modified_ms,
            create_dirs: create_directories,
        }
    }

    pub fn exec_request(command: &HostCommand) -> Request {
        Request::GitExec {
            cwd: command.cwd.clone(),
            program: command.program.clone(),
            args: command.args.clone(),
            stdin_base64: command.stdin.as_ref().map(|input| BASE64.encode(input)),
            timeout_ms: command
                .timeout
                .unwrap_or(Duration::from_secs(30))
                .as_millis() as u64,
        }
    }

    /// The daemon answers with canonical real paths (`/private/var/…` for `/var/…`).
    /// Re-root them under the path the caller asked for.
    pub fn rebase(path: &str, canonical_root: &str, requested_root: &str) -> String {
        if path == canonical_root {
            return requested_root.into();
        }
        let prefix = if canonical_root.ends_with('/') {
            canonical_root.to_string()
        } else {
            format!("{canonical_root}/")
        };
        match path.strip_prefix(&prefix) {
            Some(rest) if requested_root.ends_with('/') => format!("{requested_root}{rest}"),
            Some(rest) => format!("{requested_root}/{rest}"),
            None => path.into(),
        }
    }

    /// A `git_exec` reply as a local run would report it. Output the daemon truncated is
    /// never passed off as complete: callers with an output limit see it exceeded, every
    /// other caller gets a failure rather than a partial parse.
    pub fn command_result(response: Response, command: &HostCommand) -> CommandResult {
        match response {
            Response::GitExecResult {
                exit_code,
                stdout_base64,
                stderr_base64,
                truncated,
            } => {
                let stdout = BASE64.decode(stdout_base64).unwrap_or_default();
                let stderr = String::from_utf8(BASE64.decode(stderr_base64).unwrap_or_default())
                    .unwrap_or_default();
                if let Some(limit) = command.output_limit
                    && (stdout.len() > limit || truncated)
                {
                    return CommandResult {
                        stderr,
                        exit_code,
                        output_exceeded_limit: true,
                        ..CommandResult::default()
                    };
                }
                if truncated {
                    return CommandResult {
                        stderr: format!(
                            "Output of {} exceeded the remote daemon's limit",
                            command.program
                        ),
                        exit_code: -1,
                        output_exceeded_limit: true,
                        ..CommandResult::default()
                    };
                }
                if let Some(file) = &command.output_file {
                    if std::fs::write(file, &stdout).is_err() {
                        return CommandResult::failure("Could not save remote output", -1);
                    }
                    return CommandResult {
                        stderr,
                        exit_code,
                        ..CommandResult::default()
                    };
                }
                CommandResult {
                    stdout: String::from_utf8(stdout).unwrap_or_default(),
                    stderr,
                    exit_code,
                    output_exceeded_limit: false,
                }
            }
            Response::Error { message, .. } => CommandResult::failure(message, -1),
            _ => CommandResult::failure("Unexpected response from the remote daemon", -1),
        }
    }

    fn send(&self, request: Request) -> Result<Response, HostError> {
        self.client.send(request, Self::REQUEST_TIMEOUT)
    }

    fn failure(response: Response) -> HostError {
        match response {
            Response::Error { code, message } => HostError::from_code(&code, &message),
            other => HostError::UnexpectedResponse(response_type(&other)),
        }
    }

    fn listing(
        &self,
        request: Request,
        requested: &str,
    ) -> Result<(Vec<HostFileEntry>, bool), HostError> {
        match self.send(request)? {
            Response::FsListResult {
                path,
                entries,
                truncated,
            } => Ok((
                entries
                    .into_iter()
                    .map(|entry| HostFileEntry {
                        path: Self::rebase(&entry.path, &path, requested),
                        name: entry.name,
                        is_directory: entry.is_dir,
                        is_symlink: entry.is_symlink,
                        size: entry.size,
                        modified_ms: entry.modified_ms,
                        ignored: Some(entry.ignored),
                    })
                    .collect(),
                truncated,
            )),
            other => Err(Self::failure(other)),
        }
    }
}

/// The wire `type` of a response, for error messages.
fn response_type(response: &Response) -> String {
    serde_json::to_value(response)
        .ok()
        .and_then(|value| {
            value
                .get("type")
                .and_then(|kind| kind.as_str())
                .map(str::to_string)
        })
        .unwrap_or_else(|| "unknown".into())
}

impl<E: GitExec> ProjectHost for DaemonProjectHost<E> {
    fn is_local(&self) -> bool {
        false
    }

    fn identity(&self) -> String {
        format!("machine:{}", self.machine_id.to_lowercase())
    }

    fn list(&self, directory: &str) -> Result<Vec<HostFileEntry>, HostError> {
        Ok(self.listing(Self::list_request(directory), directory)?.0)
    }

    fn list_recursive(
        &self,
        root: &str,
        limit: usize,
    ) -> Result<(Vec<HostFileEntry>, bool), HostError> {
        self.listing(Self::list_recursive_request(root, limit), root)
    }

    fn stat(&self, path: &str) -> Result<HostFileStat, HostError> {
        match self.send(Request::FsStat { path: path.into() })? {
            Response::FsStatResult {
                exists,
                is_dir,
                size,
                modified_ms,
                ..
            } => Ok(HostFileStat {
                exists,
                is_directory: is_dir,
                size,
                modified_ms,
            }),
            Response::Error { code, .. } if code == "not_found" => Ok(HostFileStat::MISSING),
            other => Err(Self::failure(other)),
        }
    }

    fn read(&self, path: &str, max_bytes: usize) -> Result<HostFileData, HostError> {
        match self.send(Self::read_request(path, max_bytes))? {
            Response::FsReadResult {
                data_base64,
                size,
                modified_ms,
                truncated,
                ..
            } => Ok(HostFileData {
                data: BASE64
                    .decode(data_base64)
                    .map_err(|error| HostError::Io(error.to_string()))?,
                size,
                modified_ms,
                truncated,
            }),
            other => Err(Self::failure(other)),
        }
    }

    fn write(
        &self,
        path: &str,
        data: &[u8],
        expected_modified_ms: Option<i64>,
        create_directories: bool,
    ) -> Result<HostFileStat, HostError> {
        if data.len() > HostError::MAX_REMOTE_SAVE_BYTES {
            return Err(HostError::TooLargeToSave);
        }
        match self.send(Self::write_request(
            path,
            data,
            expected_modified_ms,
            create_directories,
        ))? {
            Response::FsWriteResult {
                size, modified_ms, ..
            } => Ok(HostFileStat {
                exists: true,
                is_directory: false,
                size,
                modified_ms,
            }),
            other => Err(Self::failure(other)),
        }
    }

    fn delete(&self, path: &str) -> Result<(), HostError> {
        match self.send(Request::FsDelete { path: path.into() })? {
            Response::Ok => Ok(()),
            other => Err(Self::failure(other)),
        }
    }

    fn run(&self, command: &HostCommand) -> CommandResult {
        // The daemon enforces the command's own deadline; allow for transfer on top.
        let timeout = command.timeout.unwrap_or(Duration::from_secs(30)) + Duration::from_secs(15);
        match self.client.send(Self::exec_request(command), timeout) {
            Ok(response) => Self::command_result(response, command),
            Err(error) => CommandResult::failure(error.to_string(), -1),
        }
    }
}

// MARK: - Unavailable

/// The host of a remote project that is not currently available (its machine is offline or
/// the project is gone). Every operation fails; nothing ever touches this machine's disk.
#[derive(Debug, Clone, Copy, Default)]
pub struct UnavailableProjectHost;

impl ProjectHost for UnavailableProjectHost {
    fn is_local(&self) -> bool {
        false
    }
    fn identity(&self) -> String {
        "unavailable".into()
    }
    fn list(&self, _directory: &str) -> Result<Vec<HostFileEntry>, HostError> {
        Err(HostError::unavailable())
    }
    fn list_recursive(
        &self,
        _root: &str,
        _limit: usize,
    ) -> Result<(Vec<HostFileEntry>, bool), HostError> {
        Err(HostError::unavailable())
    }
    fn stat(&self, _path: &str) -> Result<HostFileStat, HostError> {
        Err(HostError::unavailable())
    }
    fn read(&self, _path: &str, _max_bytes: usize) -> Result<HostFileData, HostError> {
        Err(HostError::unavailable())
    }
    fn write(
        &self,
        _path: &str,
        _data: &[u8],
        _expected: Option<i64>,
        _create: bool,
    ) -> Result<HostFileStat, HostError> {
        Err(HostError::unavailable())
    }
    fn delete(&self, _path: &str) -> Result<(), HostError> {
        Err(HostError::unavailable())
    }
    fn run(&self, _command: &HostCommand) -> CommandResult {
        CommandResult::failure(HostError::unavailable().to_string(), -1)
    }
}

// MARK: - Subscription backoff

/// Reconnect delays for a stream: 1 s doubling to a 15 s cap, back to 1 s once subscribed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SubscriptionBackoff {
    current: Duration,
}

impl SubscriptionBackoff {
    pub const INITIAL: Duration = Duration::from_secs(1);
    pub const MAXIMUM: Duration = Duration::from_secs(15);

    pub fn current(&self) -> Duration {
        self.current
    }

    /// The delay before the next attempt.
    pub fn next_delay(&mut self) -> Duration {
        let delay = self.current;
        self.current = (self.current * 2).min(Self::MAXIMUM);
        delay
    }

    pub fn reset(&mut self) {
        self.current = Self::INITIAL;
    }
}

impl Default for SubscriptionBackoff {
    fn default() -> Self {
        Self {
            current: Self::INITIAL,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    type Reply = Box<dyn Fn(&Request) -> Result<Response, HostError> + Send + Sync>;

    /// A daemon that records requests and answers from a closure.
    pub(crate) struct FakeGitExec {
        pub requests: Mutex<Vec<Request>>,
        pub reply: Reply,
    }

    impl GitExec for FakeGitExec {
        fn send(&self, request: Request, _timeout: Duration) -> Result<Response, HostError> {
            let reply = (self.reply)(&request);
            self.requests.lock().unwrap().push(request);
            reply
        }
    }

    fn fake(
        reply: impl Fn(&Request) -> Result<Response, HostError> + Send + Sync + 'static,
    ) -> DaemonProjectHost<FakeGitExec> {
        DaemonProjectHost::new(
            FakeGitExec {
                requests: Mutex::new(Vec::new()),
                reply: Box::new(reply),
            },
            "ABC",
        )
    }

    fn git(args: &[&str]) -> HostCommand {
        HostCommand {
            program: "git".into(),
            executable_path: "git".into(),
            args: args.iter().map(|arg| arg.to_string()).collect(),
            cwd: "/srv/repo".into(),
            ..HostCommand::default()
        }
    }

    #[test]
    fn remote_git_runs_as_a_git_exec_request() {
        let host = fake(|_| {
            Ok(Response::GitExecResult {
                exit_code: 0,
                stdout_base64: BASE64.encode("main\n"),
                stderr_base64: String::new(),
                truncated: false,
            })
        });
        let mut command = git(&["status"]);
        command.stdin = Some(b"in".to_vec());
        let result = host.run(&command);
        assert!(result.success());
        assert_eq!(result.stdout, "main\n");
        let requests = host.client.requests.lock().unwrap();
        let Request::GitExec {
            cwd,
            program,
            args,
            stdin_base64,
            timeout_ms,
        } = &requests[0]
        else {
            panic!("expected git_exec, got {:?}", requests[0]);
        };
        assert_eq!(
            (cwd.as_str(), program.as_str(), args.as_slice()),
            ("/srv/repo", "git", &["status".to_string()][..])
        );
        assert_eq!(stdin_base64.as_deref(), Some("aW4="));
        assert_eq!(*timeout_ms, 30_000);
        assert_eq!(host.identity(), "machine:abc");
    }

    #[test]
    fn truncated_remote_output_is_never_passed_off_as_complete() {
        let reply = |_: &Request| {
            Ok(Response::GitExecResult {
                exit_code: 0,
                stdout_base64: BASE64.encode("partial"),
                stderr_base64: String::new(),
                truncated: true,
            })
        };
        let host = fake(reply);
        let plain = host.run(&git(&["status"]));
        assert!(!plain.success() && plain.output_exceeded_limit && plain.stdout.is_empty());
        let mut limited = git(&["diff"]);
        limited.output_limit = Some(100);
        let limited = host.run(&limited);
        assert_eq!(
            (limited.exit_code, limited.output_exceeded_limit),
            (0, true)
        );
    }

    #[test]
    fn daemon_errors_and_the_placeholder_report_failures() {
        let host = fake(|_| {
            Ok(Response::Error {
                code: "path_not_allowed".into(),
                message: "outside".into(),
            })
        });
        assert_eq!(host.run(&git(&["status"])).stderr, "outside");
        assert_eq!(
            host.stat("/x"),
            Err(HostError::PathNotAllowed("outside".into()))
        );
        let missing = fake(|_| {
            Ok(Response::Error {
                code: "not_found".into(),
                message: "gone".into(),
            })
        });
        assert_eq!(missing.stat("/x"), Ok(HostFileStat::MISSING));

        let placeholder = DaemonProjectHost::new(UnavailableGitExec, "m");
        let result = placeholder.run(&git(&["status"]));
        assert_eq!(
            (result.exit_code, result.stderr.as_str()),
            (-1, "This project's machine is not available.")
        );
        assert!(UnavailableProjectHost.stat("/x").is_err());
    }

    #[test]
    fn remote_listings_are_rebased_on_the_requested_root() {
        type Host = DaemonProjectHost<UnavailableGitExec>;
        assert_eq!(
            Host::rebase("/private/var/x/a.txt", "/private/var/x", "/var/x"),
            "/var/x/a.txt"
        );
        assert_eq!(
            Host::rebase("/private/var/x", "/private/var/x", "/var/x"),
            "/var/x"
        );
        assert_eq!(
            Host::rebase("/elsewhere", "/private/var/x", "/var/x"),
            "/elsewhere"
        );
        let host = fake(|_| {
            Ok(Response::FsListResult {
                path: "/private/r".into(),
                entries: vec![pu_core::protocol::FsEntry {
                    name: "a".into(),
                    path: "/private/r/a".into(),
                    is_dir: false,
                    is_symlink: false,
                    size: 3,
                    modified_ms: 7,
                    ignored: true,
                }],
                truncated: false,
            })
        });
        let entries = host.list("/r").unwrap();
        assert_eq!(entries[0].path, "/r/a");
        assert_eq!(entries[0].ignored, Some(true));
        assert_eq!(host.join("/r", "b/c.txt"), "/r/b/c.txt");
        assert_eq!(host.parent("/r/b/c.txt"), "/r/b");
    }

    #[test]
    fn remote_saves_over_the_request_cap_are_refused() {
        let host = fake(|_| Ok(Response::Ok));
        let data = vec![b'x'; HostError::MAX_REMOTE_SAVE_BYTES + 1];
        assert_eq!(
            host.write("/r/a", &data, None, false),
            Err(HostError::TooLargeToSave)
        );
        assert!(host.client.requests.lock().unwrap().is_empty());
    }

    #[test]
    fn subscription_backoff_doubles_to_its_cap_and_resets() {
        let mut backoff = SubscriptionBackoff::default();
        let delays: Vec<u64> = (0..6).map(|_| backoff.next_delay().as_secs()).collect();
        assert_eq!(delays, [1, 2, 4, 8, 15, 15]);
        backoff.reset();
        assert_eq!(backoff.next_delay(), Duration::from_secs(1));
    }

    #[test]
    fn local_host_stats_reads_and_writes() {
        let dir = std::env::temp_dir().join(format!("purepoint-host-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("a.txt").to_string_lossy().into_owned();
        let host = LocalProjectHost;
        let written = host.write(&file, b"hello", None, false).unwrap();
        assert!(written.exists && written.size == 5);
        let read = host.read(&file, 3).unwrap();
        assert_eq!(
            (read.data.as_slice(), read.truncated, read.size),
            (&b"hel"[..], true, 5)
        );
        assert_eq!(
            host.write(&file, b"x", Some(written.modified_ms + 1), false),
            Err(HostError::Conflict(file.clone()))
        );
        assert!(
            host.list(&dir.to_string_lossy())
                .unwrap()
                .iter()
                .any(|entry| entry.name == "a.txt")
        );
        host.delete(&file).unwrap();
        assert!(!host.stat(&file).unwrap().exists);
        let _ = std::fs::remove_dir_all(dir);
    }
}
