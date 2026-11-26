//! The `GitService` actor from `Services/GitService.swift`: one host, its caches and the
//! git/gh process plumbing. The review operations are in `review.rs`, the PR operations in
//! `pr.rs`.
//!
//! The Swift actor serializes calls; here callers hold `&mut GitService` (the Tauri state
//! keeps each service behind a mutex).

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::host::{HostFileStat, LocalProjectHost, ProjectHost};
use super::model::{FileDiff, GitBranchReview, GitError, GitResult};
use super::runner::{self, CancelToken, CommandResult, HostCommand};

/// Disk patch snapshots a service keeps readable. Pages of older ones fail with "Refresh".
const MAX_SNAPSHOTS: usize = 128;

/// The patch files a service created. A slice from the front end is only read when it
/// names one of these, so a page request cannot read arbitrary files.
#[derive(Debug, Default)]
pub(crate) struct SnapshotStore {
    order: VecDeque<PathBuf>,
    known: HashSet<PathBuf>,
}

impl SnapshotStore {
    pub(crate) fn create(&mut self, prefix: &str) -> PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|elapsed| elapsed.as_nanos())
            .unwrap_or(0);
        let name = format!(
            "{prefix}-{}-{}-{nanos}.patch",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::SeqCst)
        );
        let path = std::env::temp_dir().join(name);
        self.order.push_back(path.clone());
        self.known.insert(path.clone());
        while self.order.len() > MAX_SNAPSHOTS {
            if let Some(old) = self.order.pop_front() {
                self.known.remove(&old);
                let _ = std::fs::remove_file(old);
            }
        }
        path
    }

    pub(crate) fn contains(&self, path: &Path) -> bool {
        self.known.contains(path)
    }
}

impl Drop for SnapshotStore {
    fn drop(&mut self) {
        for path in &self.order {
            let _ = std::fs::remove_file(path);
        }
    }
}

pub(crate) struct UntrackedEntry {
    pub stamp: String,
    pub file: FileDiff,
    pub bytes: usize,
}

pub struct GitService {
    pub(crate) host: Arc<dyn ProjectHost>,
    pub(crate) cached_gh_path: Option<String>,
    pub(crate) branch_summary_cache: HashMap<String, GitBranchReview>,
    pub(crate) untracked_cache: HashMap<String, UntrackedEntry>,
    pub(crate) untracked_cache_order: VecDeque<String>,
    pub(crate) untracked_cache_bytes: usize,
    pub(crate) snapshots: SnapshotStore,
    pub(crate) cancel: CancelToken,
    /// Finds gh when no path is cached (`locateGh`); tests swap it to simulate a missing gh.
    pub(crate) gh_lookup: fn() -> Option<String>,
}

impl Default for GitService {
    fn default() -> Self {
        Self::new(None, Arc::new(LocalProjectHost))
    }
}

impl GitService {
    pub fn new(gh_path: Option<String>, host: Arc<dyn ProjectHost>) -> Self {
        Self {
            host,
            cached_gh_path: gh_path,
            branch_summary_cache: HashMap::new(),
            untracked_cache: HashMap::new(),
            untracked_cache_order: VecDeque::new(),
            untracked_cache_bytes: 0,
            snapshots: SnapshotStore::default(),
            cancel: CancelToken::new(),
            gh_lookup: runner::locate_gh,
        }
    }

    pub fn host(&self) -> &Arc<dyn ProjectHost> {
        &self.host
    }

    /// The token checked between commands and passed to every process this service runs.
    pub fn cancel_token(&self) -> CancelToken {
        self.cancel.clone()
    }

    /// Starts a new unit of work with a fresh token (the previous one stays cancelled).
    pub fn set_cancel_token(&mut self, token: CancelToken) {
        self.cancel = token;
    }

    pub(crate) fn check_cancelled(&self) -> GitResult<()> {
        if self.cancel.is_cancelled() {
            Err(GitError::Cancelled)
        } else {
            Ok(())
        }
    }

    pub(crate) fn join(&self, directory: &str, name: &str) -> String {
        self.host.join(directory, name)
    }

    // MARK: Stamps

    pub(crate) fn stamp_of(stat: HostFileStat, include_identity: bool) -> String {
        if !stat.exists {
            return format!("-1/0/{}", if include_identity { "0" } else { "missing" });
        }
        let seconds = stat.modified_ms as f64 / 1000.0;
        let tail = if include_identity {
            stat.modified_ms.to_string()
        } else if stat.is_directory {
            "dir".into()
        } else {
            "file".into()
        };
        format!("{seconds}/{}/{tail}", stat.size)
    }

    fn stamp(&self, full_path: &str, include_identity: bool) -> String {
        Self::stamp_of(
            self.host.stat(full_path).unwrap_or(HostFileStat::MISSING),
            include_identity,
        )
    }

    /// Stats for many paths with one listing per parent directory instead of a daemon round
    /// trip per path. A directory that cannot be listed falls back to statting its paths.
    pub(crate) fn batched_stats(&self, full_paths: &[String]) -> HashMap<String, HostFileStat> {
        let mut by_directory: HashMap<String, HashSet<&String>> = HashMap::new();
        for full_path in full_paths {
            by_directory
                .entry(self.host.parent(full_path))
                .or_default()
                .insert(full_path);
        }
        let mut stats = HashMap::new();
        for (directory, wanted) in by_directory {
            if self.cancel.is_cancelled() {
                break;
            }
            let Ok(entries) = self.host.list(&directory) else {
                for full_path in wanted {
                    stats.insert(
                        full_path.clone(),
                        self.host.stat(full_path).unwrap_or(HostFileStat::MISSING),
                    );
                }
                continue;
            };
            for entry in entries {
                let full_path = self.join(&directory, &entry.name);
                if wanted.contains(&full_path) {
                    stats.insert(
                        full_path,
                        HostFileStat {
                            exists: true,
                            is_directory: entry.is_directory,
                            size: entry.size,
                            modified_ms: entry.modified_ms,
                        },
                    );
                }
            }
        }
        stats
    }

    pub(crate) fn exists(&self, full_path: &str) -> bool {
        self.host
            .stat(full_path)
            .map(|stat| stat.exists)
            .unwrap_or(false)
    }

    /// Working-file identity for preview versions: mtime, size and a file identity.
    pub(crate) fn file_stamp(&self, name: &str, root: &str) -> String {
        let full_path = self.join(root, name);
        if !self.host.is_local() {
            return self.stamp(&full_path, true);
        }
        match std::fs::symlink_metadata(&full_path) {
            Ok(metadata) => format!(
                "{}/{}/{}",
                seconds(metadata.modified().ok()),
                metadata.len(),
                file_identity(&metadata)
            ),
            Err(_) => "-1/0/0".into(),
        }
    }

    pub(crate) fn local_stamp(full_path: &str) -> String {
        match std::fs::symlink_metadata(full_path) {
            Ok(metadata) => {
                let kind = if metadata.is_dir() {
                    "directory"
                } else if metadata.file_type().is_symlink() {
                    "symlink"
                } else {
                    "regular"
                };
                format!(
                    "{}/{}/{kind}",
                    seconds(metadata.modified().ok()),
                    metadata.len()
                )
            }
            Err(_) => "-1/0/missing".into(),
        }
    }

    // MARK: Processes

    /// git with literal pathspecs, through the host.
    pub fn run_git(&self, args: &[&str], cwd: &str, output_limit: Option<usize>) -> CommandResult {
        self.host.run(&self.git_command(
            args.iter().map(|arg| arg.to_string()).collect(),
            cwd,
            output_limit,
        ))
    }

    pub(crate) fn run_git_owned(
        &self,
        args: Vec<String>,
        cwd: &str,
        output_limit: Option<usize>,
    ) -> CommandResult {
        self.host.run(&self.git_command(args, cwd, output_limit))
    }

    pub(crate) fn git_command(
        &self,
        args: Vec<String>,
        cwd: &str,
        output_limit: Option<usize>,
    ) -> HostCommand {
        let mut all = vec!["--literal-pathspecs".to_string()];
        all.extend(args);
        HostCommand {
            program: "git".into(),
            executable_path: runner::git_path().into(),
            args: all,
            cwd: cwd.into(),
            output_limit,
            cancel: Some(self.cancel.clone()),
            ..HostCommand::default()
        }
    }

    /// A git command that must succeed; its stdout. Stops first when cancelled.
    pub(crate) fn checked(&self, args: &[&str], root: &str) -> GitResult<String> {
        self.checked_owned(args.iter().map(|arg| arg.to_string()).collect(), root)
    }

    pub(crate) fn checked_owned(&self, args: Vec<String>, root: &str) -> GitResult<String> {
        self.check_cancelled()?;
        let result = self.run_git_owned(args, root, None);
        if !result.success() {
            let message = result.stderr.trim();
            return Err(GitError::Review(if message.is_empty() {
                format!(
                    "Git command failed (exit {}); the refs may have no common ancestor.",
                    result.exit_code
                )
            } else {
                message.to_string()
            }));
        }
        Ok(result.stdout)
    }

    /// gh through the host. A remote machine's daemon runs its own gh with that user's auth;
    /// locally gh is found once and cached. A missing gh reports `gh not found`, as on the Mac.
    pub(crate) fn run_gh(
        &mut self,
        args: &[&str],
        cwd: &str,
        output_file: Option<PathBuf>,
    ) -> CommandResult {
        let args: Vec<String> = args.iter().map(|arg| arg.to_string()).collect();
        let mut command = HostCommand {
            program: "gh".into(),
            executable_path: "gh".into(),
            args,
            cwd: cwd.into(),
            timeout: Some(Duration::from_secs(25)),
            output_file,
            cancel: Some(self.cancel.clone()),
            ..HostCommand::default()
        };
        if self.host.is_local() {
            let path = match &self.cached_gh_path {
                Some(path) => path.clone(),
                None => match (self.gh_lookup)() {
                    Some(path) => {
                        self.cached_gh_path = Some(path.clone());
                        path
                    }
                    None => return CommandResult::failure("gh not found", 1),
                },
            };
            command.executable_path = path;
        }
        self.host.run(&command)
    }
}

fn seconds(time: Option<SystemTime>) -> String {
    match time.and_then(|time| time.duration_since(UNIX_EPOCH).ok()) {
        Some(elapsed) => elapsed.as_secs_f64().to_string(),
        None => "-1".into(),
    }
}

/// Stands in for the Mac's `systemFileNumber`: the inode on Unix, the creation time on
/// Windows (std has no stable file index there).
fn file_identity(metadata: &std::fs::Metadata) -> String {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        metadata.ino().to_string()
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.creation_time().to_string()
    }
}
