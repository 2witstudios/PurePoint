//! Daemon file and git requests (MACH-007): every path is canonicalized and
//! confined to a registered project root, one of its worktrees, or an agent
//! transcript directory under the daemon user's home.

use std::collections::VecDeque;
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, UNIX_EPOCH};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use pu_core::manifest;
use pu_core::protocol::{FsEntry, Request, Response};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};

use super::Engine;

/// Agent transcript directories, relative to the daemon user's `$HOME`.
const TRANSCRIPT_DIRS: [&str; 3] = [
    ".claude/projects",
    ".codex/sessions",
    ".local/share/opencode",
];
/// Per-stream cap for `git_exec` stdout and stderr.
const GIT_EXEC_OUTPUT_CAP: usize = 8 * 1024 * 1024;
const GIT_EXEC_PROGRAMS: [&str; 2] = ["git", "gh"];

fn fs_error(code: &str, message: impl Into<String>) -> Response {
    Response::Error {
        code: code.into(),
        message: message.into(),
    }
}

fn not_allowed(path: &Path) -> Response {
    fs_error(
        "path_not_allowed",
        format!(
            "{} is outside registered projects, their worktrees and agent transcript directories",
            path.display()
        ),
    )
}

fn io_error(path: &Path, e: &std::io::Error) -> Response {
    match e.kind() {
        std::io::ErrorKind::NotFound => {
            fs_error("not_found", format!("{} does not exist", path.display()))
        }
        _ => fs_error("io_error", format!("{}: {e}", path.display())),
    }
}

fn modified_ms(meta: &std::fs::Metadata) -> i64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

fn path_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

/// Canonical directories fs requests may touch.
fn allowed_roots(projects: &[String], home: Option<&Path>) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    for project in projects {
        let root = Path::new(project);
        let Ok(canonical) = root.canonicalize() else {
            continue;
        };
        roots.push(canonical);
        if let Ok(m) = manifest::read_manifest(root) {
            roots.extend(
                m.worktrees
                    .values()
                    .filter_map(|wt| Path::new(&wt.path).canonicalize().ok()),
            );
        }
    }
    if let Some(home) = home {
        roots.extend(
            TRANSCRIPT_DIRS
                .iter()
                .filter_map(|dir| home.join(dir).canonicalize().ok()),
        );
    }
    roots
}

/// Resolves `path` to a real path inside `roots`.
///
/// The existing prefix is canonicalized (resolving `..` and symlinks); a
/// missing tail may only contain plain names. With `follow_leaf == false`
/// the final component is kept as-is so a symlink itself can be removed.
#[allow(clippy::result_large_err)]
fn resolve_confined(path: &str, roots: &[PathBuf], follow_leaf: bool) -> Result<PathBuf, Response> {
    let input = Path::new(path);
    if !input.is_absolute() {
        return Err(fs_error(
            "path_not_allowed",
            format!("{path} is not an absolute path"),
        ));
    }
    let resolved = if follow_leaf {
        resolve_existing_prefix(input)?
    } else {
        let name = match input.components().next_back() {
            Some(Component::Normal(name)) => name.to_owned(),
            _ => return Err(not_allowed(input)),
        };
        let parent = input.parent().ok_or_else(|| not_allowed(input))?;
        resolve_existing_prefix(parent)?.join(name)
    };
    if roots.iter().any(|root| resolved.starts_with(root)) {
        Ok(resolved)
    } else {
        Err(not_allowed(&resolved))
    }
}

#[allow(clippy::result_large_err)]
fn resolve_existing_prefix(input: &Path) -> Result<PathBuf, Response> {
    for ancestor in input.ancestors() {
        let Ok(canonical) = ancestor.canonicalize() else {
            continue;
        };
        let tail = input
            .strip_prefix(ancestor)
            .map_err(|_| not_allowed(input))?;
        let mut resolved = canonical;
        for (i, component) in tail.components().enumerate() {
            match component {
                Component::Normal(name) => resolved.push(name),
                Component::CurDir => {}
                _ => return Err(not_allowed(input)),
            }
            // Only a dangling symlink can exist below the deepest resolvable
            // ancestor; following it could land anywhere.
            if i == 0 && std::fs::symlink_metadata(&resolved).is_ok() {
                return Err(not_allowed(input));
            }
        }
        return Ok(resolved);
    }
    Err(not_allowed(input))
}

/// Paths (of `candidates`) that git ignores in the repository containing `dir`.
/// Non-repositories and git failures report nothing as ignored.
fn git_ignored(dir: &Path, candidates: &[String]) -> std::collections::HashSet<String> {
    let mut ignored = std::collections::HashSet::new();
    if candidates.is_empty() {
        return ignored;
    }
    let child = std::process::Command::new("git")
        .args(["check-ignore", "--stdin", "-z"])
        .current_dir(dir)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn();
    let Ok(mut child) = child else {
        return ignored;
    };
    let mut input = Vec::new();
    for path in candidates {
        input.extend_from_slice(path.as_bytes());
        input.push(0);
    }
    let Some(mut stdin) = child.stdin.take() else {
        let _ = child.kill();
        let _ = child.wait();
        return ignored;
    };
    // Write from a separate thread so a full stdout pipe can't deadlock us.
    let writer = std::thread::spawn(move || {
        let _ = stdin.write_all(&input);
    });
    let mut output = Vec::new();
    if let Some(mut stdout) = child.stdout.take() {
        let _ = stdout.read_to_end(&mut output);
    }
    let _ = writer.join();
    let _ = child.wait();
    for path in output.split(|&b| b == 0).filter(|p| !p.is_empty()) {
        ignored.insert(String::from_utf8_lossy(path).into_owned());
    }
    ignored
}

fn fs_list(
    path: &str,
    roots: &[PathBuf],
    recursive: bool,
    include_hidden: bool,
    max_entries: usize,
) -> Response {
    let dir = match resolve_confined(path, roots, true) {
        Ok(dir) => dir,
        Err(e) => return e,
    };
    match std::fs::metadata(&dir) {
        Ok(meta) if meta.is_dir() => {}
        Ok(_) => {
            return fs_error(
                "not_a_directory",
                format!("{} is not a directory", dir.display()),
            );
        }
        Err(e) => return io_error(&dir, &e),
    }

    let mut entries = Vec::new();
    let mut truncated = false;
    let mut queue = VecDeque::from([dir.clone()]);
    'walk: while let Some(current) = queue.pop_front() {
        let Ok(read) = std::fs::read_dir(&current) else {
            // Unreadable subdirectories are skipped; only the requested one must open.
            if current == dir {
                return fs_error("io_error", format!("cannot read {}", dir.display()));
            }
            continue;
        };
        let mut children: Vec<_> = read.filter_map(Result::ok).collect();
        children.sort_by_key(|e| e.file_name());
        for child in children {
            let name = child.file_name().to_string_lossy().into_owned();
            if !include_hidden && name.starts_with('.') {
                continue;
            }
            if entries.len() >= max_entries {
                truncated = true;
                break 'walk;
            }
            let child_path = child.path();
            let Ok(link_meta) = std::fs::symlink_metadata(&child_path) else {
                continue;
            };
            let is_symlink = link_meta.file_type().is_symlink();
            let meta = if is_symlink {
                std::fs::metadata(&child_path).unwrap_or(link_meta)
            } else {
                link_meta
            };
            let is_dir = meta.is_dir();
            // Never descend through symlinks (cycles, escapes) or into `.git`.
            if recursive && is_dir && !is_symlink && name != ".git" {
                queue.push_back(child_path.clone());
            }
            entries.push(FsEntry {
                name,
                path: path_string(&child_path),
                is_dir,
                is_symlink,
                size: if is_dir { 0 } else { meta.len() },
                modified_ms: modified_ms(&meta),
                ignored: false,
            });
        }
    }

    let candidates: Vec<String> = entries.iter().map(|e| e.path.clone()).collect();
    let ignored = git_ignored(&dir, &candidates);
    if !ignored.is_empty() {
        for entry in &mut entries {
            entry.ignored = ignored.contains(&entry.path);
        }
    }
    Response::FsListResult {
        path: path_string(&dir),
        entries,
        truncated,
    }
}

fn fs_stat(path: &str, roots: &[PathBuf]) -> Response {
    let resolved = match resolve_confined(path, roots, true) {
        Ok(p) => p,
        Err(e) => return e,
    };
    match std::fs::metadata(&resolved) {
        Ok(meta) => Response::FsStatResult {
            path: path_string(&resolved),
            exists: true,
            is_dir: meta.is_dir(),
            size: if meta.is_dir() { 0 } else { meta.len() },
            modified_ms: modified_ms(&meta),
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Response::FsStatResult {
            path: path_string(&resolved),
            exists: false,
            is_dir: false,
            size: 0,
            modified_ms: 0,
        },
        Err(e) => io_error(&resolved, &e),
    }
}

fn fs_read(path: &str, roots: &[PathBuf], max_bytes: u64) -> Response {
    let resolved = match resolve_confined(path, roots, true) {
        Ok(p) => p,
        Err(e) => return e,
    };
    let file = match std::fs::File::open(&resolved) {
        Ok(f) => f,
        Err(e) => return io_error(&resolved, &e),
    };
    let meta = match file.metadata() {
        Ok(m) => m,
        Err(e) => return io_error(&resolved, &e),
    };
    if meta.is_dir() {
        return fs_error(
            "is_directory",
            format!("{} is a directory", resolved.display()),
        );
    }
    let mut data = Vec::new();
    if let Err(e) = file.take(max_bytes).read_to_end(&mut data) {
        return io_error(&resolved, &e);
    }
    Response::FsReadResult {
        path: path_string(&resolved),
        data_base64: BASE64_STANDARD.encode(&data),
        size: meta.len(),
        modified_ms: modified_ms(&meta),
        truncated: meta.len() > data.len() as u64,
    }
}

fn fs_write(
    path: &str,
    roots: &[PathBuf],
    data_base64: &str,
    expected_modified_ms: Option<i64>,
    create_dirs: bool,
) -> Response {
    let resolved = match resolve_confined(path, roots, true) {
        Ok(p) => p,
        Err(e) => return e,
    };
    let data = match BASE64_STANDARD.decode(data_base64) {
        Ok(d) => d,
        Err(e) => return fs_error("invalid_base64", format!("data_base64: {e}")),
    };
    let existing = match std::fs::metadata(&resolved) {
        Ok(meta) => Some(meta),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return io_error(&resolved, &e),
    };
    if existing.as_ref().is_some_and(|m| m.is_dir()) {
        return fs_error(
            "is_directory",
            format!("{} is a directory", resolved.display()),
        );
    }
    if let Some(expected) = expected_modified_ms {
        let current = existing.as_ref().map(modified_ms);
        if current != Some(expected) {
            return fs_error(
                "conflict",
                format!(
                    "{} changed since it was read (expected modified_ms {expected}, found {})",
                    resolved.display(),
                    current.map_or_else(|| "missing file".into(), |c| c.to_string())
                ),
            );
        }
    }
    let Some(parent) = resolved.parent() else {
        return not_allowed(&resolved);
    };
    if create_dirs && let Err(e) = std::fs::create_dir_all(parent) {
        return io_error(parent, &e);
    }
    if let Err(e) = write_atomically(&resolved, parent, &data, existing.as_ref()) {
        return io_error(&resolved, &e);
    }
    match std::fs::metadata(&resolved) {
        Ok(meta) => Response::FsWriteResult {
            path: path_string(&resolved),
            size: meta.len(),
            modified_ms: modified_ms(&meta),
        },
        Err(e) => io_error(&resolved, &e),
    }
}

/// Writes through a sibling temp file and rename, so readers never see a
/// half-written file. Keeps the existing file's permissions.
fn write_atomically(
    target: &Path,
    parent: &Path,
    data: &[u8],
    existing: Option<&std::fs::Metadata>,
) -> std::io::Result<()> {
    let name = target
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let nonce = std::time::SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let tmp = parent.join(format!(".{name}.pu-write-{}-{nonce}", std::process::id()));
    let result = (|| {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp)?;
        file.write_all(data)?;
        file.sync_all()?;
        if let Some(meta) = existing {
            std::fs::set_permissions(&tmp, meta.permissions())?;
        }
        std::fs::rename(&tmp, target)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

fn fs_delete(path: &str, roots: &[PathBuf]) -> Response {
    let resolved = match resolve_confined(path, roots, false) {
        Ok(p) => p,
        Err(e) => return e,
    };
    // A root itself (project, worktree, transcript dir) is never deletable here.
    if roots.iter().any(|root| root == &resolved) {
        return not_allowed(&resolved);
    }
    let meta = match std::fs::symlink_metadata(&resolved) {
        Ok(m) => m,
        Err(e) => return io_error(&resolved, &e),
    };
    let result = if meta.is_dir() {
        std::fs::remove_dir_all(&resolved)
    } else {
        std::fs::remove_file(&resolved)
    };
    match result {
        Ok(()) => Response::Ok,
        Err(e) => io_error(&resolved, &e),
    }
}

/// Reads a stream to the end, keeping at most `cap` bytes.
async fn read_capped<R: AsyncRead + Unpin>(mut reader: R, cap: usize) -> (Vec<u8>, bool) {
    let mut kept = Vec::new();
    let mut truncated = false;
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        match reader.read(&mut buf).await {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                let room = cap.saturating_sub(kept.len());
                if n > room {
                    truncated = true;
                }
                kept.extend_from_slice(&buf[..n.min(room)]);
            }
        }
    }
    (kept, truncated)
}

impl Engine {
    /// Directories fs and git requests may touch right now.
    async fn fs_roots(&self) -> Vec<PathBuf> {
        let projects = self.registered_projects();
        tokio::task::spawn_blocking(move || {
            let home = std::env::var_os("HOME").map(PathBuf::from);
            allowed_roots(&projects, home.as_deref())
        })
        .await
        .unwrap_or_default()
    }

    /// Canonical, confined, existing directory for `subscribe_fs` and `git_exec`.
    pub(crate) async fn resolve_fs_dir(&self, path: &str) -> Result<PathBuf, Response> {
        let roots = self.fs_roots().await;
        let path = path.to_string();
        tokio::task::spawn_blocking(move || {
            let dir = resolve_confined(&path, &roots, true)?;
            match std::fs::metadata(&dir) {
                Ok(meta) if meta.is_dir() => Ok(dir),
                Ok(_) => Err(fs_error(
                    "not_a_directory",
                    format!("{} is not a directory", dir.display()),
                )),
                Err(e) => Err(io_error(&dir, &e)),
            }
        })
        .await
        .unwrap_or_else(|e| Err(fs_error("INTERNAL_ERROR", format!("task join error: {e}"))))
    }

    pub(super) async fn handle_fs_request(&self, request: Request) -> Response {
        match request {
            Request::SubscribeFs { root, .. } => {
                return match self.resolve_fs_dir(&root).await {
                    Ok(_) => Response::FsSubscribed,
                    Err(e) => e,
                };
            }
            Request::GitExec {
                cwd,
                program,
                args,
                stdin_base64,
                timeout_ms,
            } => {
                return self
                    .handle_git_exec(&cwd, &program, args, stdin_base64, timeout_ms)
                    .await;
            }
            _ => {}
        }
        let roots = self.fs_roots().await;
        tokio::task::spawn_blocking(move || match request {
            Request::FsList {
                path,
                recursive,
                include_hidden,
                max_entries,
            } => fs_list(&path, &roots, recursive, include_hidden, max_entries),
            Request::FsStat { path } => fs_stat(&path, &roots),
            Request::FsRead { path, max_bytes } => fs_read(&path, &roots, max_bytes),
            Request::FsWrite {
                path,
                data_base64,
                expected_modified_ms,
                create_dirs,
            } => fs_write(
                &path,
                &roots,
                &data_base64,
                expected_modified_ms,
                create_dirs,
            ),
            Request::FsDelete { path } => fs_delete(&path, &roots),
            other => fs_error("INTERNAL_ERROR", format!("not a file request: {other:?}")),
        })
        .await
        .unwrap_or_else(|e| fs_error("INTERNAL_ERROR", format!("task join error: {e}")))
    }

    async fn handle_git_exec(
        &self,
        cwd: &str,
        program: &str,
        args: Vec<String>,
        stdin_base64: Option<String>,
        timeout_ms: u64,
    ) -> Response {
        if !GIT_EXEC_PROGRAMS.contains(&program) {
            return fs_error(
                "program_not_allowed",
                format!("git_exec runs only git or gh, not {program:?}"),
            );
        }
        let stdin_data = match stdin_base64.as_deref().map(|s| BASE64_STANDARD.decode(s)) {
            Some(Ok(data)) => Some(data),
            Some(Err(e)) => return fs_error("invalid_base64", format!("stdin_base64: {e}")),
            None => None,
        };
        let cwd = match self.resolve_fs_dir(cwd).await {
            Ok(dir) => dir,
            Err(e) => return e,
        };
        // The daemon may run under launchd with a minimal PATH; agents get the
        // login shell's, and so does gh/git here.
        let login_path = self
            .login_env
            .get_or_init(Self::resolve_login_env)
            .await
            .iter()
            .find(|(k, _)| k == "PATH")
            .map(|(_, v)| v.clone());

        let mut command = tokio::process::Command::new(program);
        command
            .args(&args)
            .current_dir(&cwd)
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GH_PROMPT_DISABLED", "1")
            .stdin(if stdin_data.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        if let Some(path) = login_path {
            command.env("PATH", path);
        }
        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(e) => return fs_error("io_error", format!("failed to run {program}: {e}")),
        };
        if let (Some(data), Some(mut stdin)) = (stdin_data, child.stdin.take()) {
            tokio::spawn(async move {
                let _ = stdin.write_all(&data).await;
            });
        }
        let (Some(stdout), Some(stderr)) = (child.stdout.take(), child.stderr.take()) else {
            return fs_error("io_error", format!("failed to capture {program} output"));
        };
        let run = async {
            let ((out, out_cut), (err, err_cut), status) = tokio::join!(
                read_capped(stdout, GIT_EXEC_OUTPUT_CAP),
                read_capped(stderr, GIT_EXEC_OUTPUT_CAP),
                child.wait()
            );
            (out, err, out_cut || err_cut, status)
        };
        match tokio::time::timeout(Duration::from_millis(timeout_ms), run).await {
            Ok((stdout, stderr, truncated, Ok(status))) => Response::GitExecResult {
                exit_code: status.code().unwrap_or(-1),
                stdout_base64: BASE64_STANDARD.encode(stdout),
                stderr_base64: BASE64_STANDARD.encode(stderr),
                truncated,
            },
            Ok((_, _, _, Err(e))) => fs_error("io_error", format!("{program} failed: {e}")),
            Err(_) => {
                let _ = child.start_kill();
                fs_error(
                    "timeout",
                    format!("{program} did not finish within {timeout_ms} ms"),
                )
            }
        }
    }
}

#[cfg(test)]
mod tests;
