//! Ports of `purepoint-macosTests/GitReviewTests.swift` (`git_review.rs`) and
//! `WorkingTreeDiffTests.swift` (`working_tree_diff.rs`), run against real temporary git
//! repositories as the Swift tests are.
//!
//! NTFS cannot hold `"`, tab or newline in a file name, so the cases that create such files
//! use names git still C-quotes (non-ASCII, spaces, brackets, apostrophes). Patches that only
//! carry such names (the PR patches) keep the Mac's exact names.

mod git_review;
mod working_tree_diff;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use super::host::{
    HostError, HostFileData, HostFileEntry, HostFileStat, LocalProjectHost, ProjectHost,
};
use super::runner::{self, CommandResult, HostCommand};
use super::service::GitService;

/// A temporary directory removed on drop.
pub(crate) struct TempDir(PathBuf);

impl TempDir {
    pub fn new(prefix: &str) -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "{prefix}-{}-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::SeqCst),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }

    pub fn str(&self) -> String {
        self.0.to_string_lossy().into_owned()
    }

    pub fn file(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        // git marks objects read-only; clear that so the tree can go.
        fn writable(path: &Path) {
            if let Ok(entries) = std::fs::read_dir(path) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if let Ok(metadata) = std::fs::symlink_metadata(&path) {
                        if metadata.is_dir() {
                            writable(&path);
                        } else {
                            let mut permissions = metadata.permissions();
                            #[allow(clippy::permissions_set_readonly_false)]
                            permissions.set_readonly(false);
                            let _ = std::fs::set_permissions(&path, permissions);
                        }
                    }
                }
            }
        }
        writable(&self.0);
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// git in `at`; its trimmed combined output, or that output as the error.
pub(crate) fn try_git(args: &[&str], at: &Path) -> Result<String, String> {
    let mut command = Command::new(runner::git_path());
    command.args(args).current_dir(at);
    runner::hide_console(&mut command);
    let output = command.output().map_err(|error| error.to_string())?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    if output.status.success() {
        Ok(text.trim().to_string())
    } else {
        Err(format!("git {args:?}: {text}"))
    }
}

pub(crate) fn git(args: &[&str], at: &Path) -> String {
    try_git(args, at).unwrap_or_else(|error| panic!("{error}"))
}

/// A merge that must fail with real conflicts.
pub(crate) fn conflicting_merge(branch: &str, at: &Path) {
    let error = try_git(&["merge", branch], at).expect_err("merge unexpectedly succeeded");
    assert!(error.contains("CONFLICT"), "real unresolved merge: {error}");
}

/// `git init -b main` with an identity and no line-ending conversion.
pub(crate) fn repo() -> TempDir {
    let dir = TempDir::new("purepoint-git");
    git(&["init", "-b", "main"], dir.path());
    git(&["config", "user.email", "test@example.test"], dir.path());
    git(&["config", "user.name", "Test"], dir.path());
    git(&["config", "core.autocrlf", "false"], dir.path());
    dir
}

pub(crate) fn write(value: impl AsRef<[u8]>, name: &str, at: &Path) {
    let path = at.join(name);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, value).unwrap();
}

pub(crate) fn commit(message: &str, at: &Path) {
    git(&["add", "--all"], at);
    git(&["commit", "-m", message], at);
}

/// The fake `gh` of the Swift tests, as a host: git runs for real, gh answers from files in
/// its working directory (`prs.json`, `pr.patch`) and fails while `fail`/`fail-diff` exist.
pub(crate) struct FakeGhHost {
    /// Replaces gh's stderr and exit code when set (signed-out gh).
    pub failure: Option<(String, i32)>,
}

impl FakeGhHost {
    pub fn shared() -> Arc<dyn ProjectHost> {
        Arc::new(Self { failure: None })
    }

    fn gh(&self, command: &HostCommand) -> CommandResult {
        if let Some((stderr, code)) = &self.failure {
            return CommandResult::failure(stderr.clone(), *code);
        }
        let cwd = Path::new(&command.cwd);
        if cwd.join("fail").exists() {
            return CommandResult::failure("remote-unavailable\n", 1);
        }
        let output = if command.args.get(1).map(String::as_str) == Some("list") {
            std::fs::read(cwd.join("prs.json")).unwrap_or_default()
        } else {
            if cwd.join("fail-diff").exists() {
                return CommandResult::failure("temporary-diff-error\n", 1);
            }
            std::fs::read(cwd.join("pr.patch")).unwrap_or_default()
        };
        if let Some(file) = &command.output_file {
            std::fs::write(file, output).unwrap();
            return CommandResult::default();
        }
        CommandResult {
            stdout: String::from_utf8(output).unwrap(),
            ..CommandResult::default()
        }
    }
}

impl ProjectHost for FakeGhHost {
    fn is_local(&self) -> bool {
        true
    }
    fn identity(&self) -> String {
        "local".into()
    }
    fn list(&self, directory: &str) -> Result<Vec<HostFileEntry>, HostError> {
        LocalProjectHost.list(directory)
    }
    fn list_recursive(
        &self,
        root: &str,
        limit: usize,
    ) -> Result<(Vec<HostFileEntry>, bool), HostError> {
        LocalProjectHost.list_recursive(root, limit)
    }
    fn stat(&self, path: &str) -> Result<HostFileStat, HostError> {
        LocalProjectHost.stat(path)
    }
    fn read(&self, path: &str, max_bytes: usize) -> Result<HostFileData, HostError> {
        LocalProjectHost.read(path, max_bytes)
    }
    fn write(
        &self,
        path: &str,
        data: &[u8],
        expected: Option<i64>,
        create: bool,
    ) -> Result<HostFileStat, HostError> {
        LocalProjectHost.write(path, data, expected, create)
    }
    fn delete(&self, path: &str) -> Result<(), HostError> {
        LocalProjectHost.delete(path)
    }
    fn run(&self, command: &HostCommand) -> CommandResult {
        if command.program == "gh" {
            self.gh(command)
        } else {
            runner::run_local(command)
        }
    }
}

/// A service whose gh is the fake.
pub(crate) fn fake_gh_service() -> GitService {
    GitService::new(Some("fake-gh".into()), FakeGhHost::shared())
}
