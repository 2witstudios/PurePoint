//! Daemon file and git requests (MACH-007): every path is canonicalized and
//! confined to a registered project root, one of its worktrees, or an agent
//! transcript directory under the daemon user's home.

use std::collections::VecDeque;
use std::io::{Read, Write};
use std::os::unix::fs::{OpenOptionsExt as _, PermissionsExt as _};
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
/// Upper bound on `fs_read`'s `max_bytes`, whatever the client asks for.
const FS_READ_MAX_BYTES: u64 = 8 * 1024 * 1024;
/// Upper bound on `fs_list`'s `max_entries`.
const FS_LIST_MAX_ENTRIES: usize = 100_000;
/// Upper bound on `git_exec`'s `timeout_ms`.
const GIT_EXEC_MAX_TIMEOUT_MS: u64 = 120_000;
/// How long `fs_list` waits for `git check-ignore`.
const CHECK_IGNORE_TIMEOUT: Duration = Duration::from_secs(10);
/// Prepended to every `git_exec` git command (after the client's arguments
/// are checked): repository config can't run an fsmonitor hook or hooks.
/// `safe.bareRepository=explicit`: a directory a client planted with `HEAD`,
/// `objects/`, `refs/` and a `config` is never taken for a repository.
const GIT_EXEC_PREFIX: [&str; 6] = [
    "-c",
    "core.fsmonitor=",
    "-c",
    "core.hooksPath=/dev/null",
    "-c",
    "safe.bareRepository=explicit",
];

/// Global options git may receive before its subcommand. Everything else that
/// redirects the repository, config or helper programs (`-C`, `--git-dir`,
/// `--work-tree`, `--exec-path`, `--namespace`, `--config-env`, …) is refused.
const GIT_GLOBAL_FLAGS: [&str; 7] = [
    "--literal-pathspecs",
    "--glob-pathspecs",
    "--noglob-pathspecs",
    "--icase-pathspecs",
    "--no-pager",
    "--no-optional-locks",
    "--no-replace-objects",
];
/// Config keys `-c` may set (lowercase): display-only settings.
const GIT_CONFIG_KEYS: [&str; 1] = ["core.quotepath"];
/// The read-only subcommands `GitService.swift` runs. A positive allowlist:
/// aliases, writers (`config`, `worktree`, `init`, `bundle`, `format-patch`,
/// …) and any subcommand git adds later are refused.
const GIT_ALLOWED_SUBCOMMANDS: [&str; 10] = [
    "status",
    "diff",
    "diff-tree",
    "log",
    "rev-parse",
    "rev-list",
    "merge-base",
    "ls-files",
    "for-each-ref",
    "symbolic-ref",
];
/// `gh` commands the app runs; extensions and aliases are refused.
/// gh commands the app runs (`GitService.swift`), with the options each may take.
const GH_ALLOWED: [(&str, &[&str]); 3] = [
    ("pr", &["list", "diff"]),
    ("auth", &["status"]),
    ("repo", &["view"]),
];

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

/// A manifest worktree path is trusted only where the daemon creates
/// worktrees: strictly inside `<root>/.pu/worktrees/` (canonicalized), and
/// never `/` or an ancestor of the project root or `$HOME`.
fn trusted_worktree(
    worktree: &Path,
    worktrees_dir: &Path,
    project: &Path,
    home: Option<&Path>,
) -> Option<PathBuf> {
    let canonical = worktree.canonicalize().ok()?;
    let inside = canonical.starts_with(worktrees_dir) && canonical != worktrees_dir;
    let too_broad = canonical.parent().is_none()
        || project.starts_with(&canonical)
        || home.is_some_and(|home| home.starts_with(&canonical));
    (inside && !too_broad).then_some(canonical)
}

/// Canonical directories fs requests may touch. Agent transcript
/// directories are read-only: `fs_write` and `fs_delete` use `writable`.
#[derive(Debug, Default)]
struct FsRoots {
    /// Project roots and their worktrees.
    writable: Vec<PathBuf>,
    /// `writable` plus the agent transcript directories.
    all: Vec<PathBuf>,
}

fn allowed_roots(projects: &[String], home: Option<&Path>) -> FsRoots {
    let mut roots = Vec::new();
    for project in projects {
        let root = Path::new(project);
        let Ok(canonical) = root.canonicalize() else {
            continue;
        };
        // However it got registered (legacy registry entry, auto-registration of a
        // directory with a `.pu/manifest.json` such as `$HOME`), a root this broad
        // would expose the whole home directory: it is never an fs root.
        if is_too_broad_project_root(&canonical, home) {
            continue;
        }
        let worktrees_dir = pu_core::paths::pu_dir(root)
            .join("worktrees")
            .canonicalize()
            .ok();
        if let (Some(worktrees_dir), Ok(m)) = (worktrees_dir, manifest::read_manifest(root)) {
            roots.extend(m.worktrees.values().filter_map(|wt| {
                trusted_worktree(Path::new(&wt.path), &worktrees_dir, &canonical, home)
            }));
        }
        roots.push(canonical);
    }
    let mut all = roots.clone();
    if let Some(home) = home {
        all.extend(
            TRANSCRIPT_DIRS
                .iter()
                .filter_map(|dir| home.join(dir).canonicalize().ok()),
        );
    }
    FsRoots {
        writable: roots,
        all,
    }
}

/// Whether `project_root` is too broad to register: `/`, `$HOME`, or an
/// ancestor of `$HOME` (registering it would expose the whole home directory
/// to fs requests).
pub(crate) fn is_too_broad_project_root(project_root: &Path, home: Option<&Path>) -> bool {
    let root = project_root
        .canonicalize()
        .unwrap_or_else(|_| project_root.to_path_buf());
    if root.parent().is_none() {
        return true;
    }
    home.is_some_and(|home| {
        let home = home.canonicalize().unwrap_or_else(|_| home.to_path_buf());
        home.starts_with(&root)
    })
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
fn git_ignored(
    dir: &Path,
    roots: &[PathBuf],
    candidates: &[String],
) -> std::collections::HashSet<String> {
    let mut ignored = std::collections::HashSet::new();
    if candidates.is_empty() {
        return ignored;
    }
    use std::os::unix::process::CommandExt as _;
    // The same hardening as `git_exec`: a listing must not run what a planted
    // or cloned bare repository configures (fsmonitor, hooks, ...).
    let mut command = std::process::Command::new("git");
    command
        .args(exec_args(
            "git",
            strings_of(&["check-ignore", "--stdin", "-z"]),
        ))
        .envs(exec_env("git"));
    if let Some(ceiling) = git_ceiling(dir, roots) {
        command.env("GIT_CEILING_DIRECTORIES", ceiling);
    }
    let child = command
        .current_dir(dir)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        // Own process group, so a timeout also stops anything git spawned.
        .process_group(0)
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
    let reader = child.stdout.take().map(|mut stdout| {
        std::thread::spawn(move || {
            let mut output = Vec::new();
            let _ = stdout.read_to_end(&mut output);
            output
        })
    });
    let deadline = std::time::Instant::now() + CHECK_IGNORE_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if std::time::Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(10));
            }
            _ => {
                // Timed out (or can't wait): report nothing as ignored.
                kill_process_group(child.id());
                let _ = child.kill();
                let _ = child.wait();
                let _ = writer.join();
                if let Some(reader) = reader {
                    let _ = reader.join();
                }
                return ignored;
            }
        }
    }
    let _ = writer.join();
    let output = reader
        .and_then(|reader| reader.join().ok())
        .unwrap_or_default();
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
    let ignored = git_ignored(&dir, roots, &candidates);
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
    // Check before opening: opening a FIFO blocks until a writer appears.
    match std::fs::metadata(&resolved) {
        Ok(meta) => {
            if let Err(response) = check_regular_file(&resolved, &meta) {
                return response;
            }
        }
        Err(e) => return io_error(&resolved, &e),
    }
    // O_NONBLOCK too, in case the path was swapped for a FIFO since.
    let file = match std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(&resolved)
    {
        Ok(f) => f,
        Err(e) => return io_error(&resolved, &e),
    };
    let meta = match file.metadata() {
        Ok(m) => m,
        Err(e) => return io_error(&resolved, &e),
    };
    if let Err(response) = check_regular_file(&resolved, &meta) {
        return response;
    }
    let mut data = Vec::new();
    if let Err(e) = file
        .take(max_bytes.min(FS_READ_MAX_BYTES))
        .read_to_end(&mut data)
    {
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

/// `fs_read` serves regular files only (not FIFOs, sockets or devices).
#[allow(clippy::result_large_err)]
fn check_regular_file(path: &Path, meta: &std::fs::Metadata) -> Result<(), Response> {
    if meta.is_dir() {
        Err(fs_error(
            "is_directory",
            format!("{} is a directory", path.display()),
        ))
    } else if !meta.is_file() {
        Err(fs_error(
            "not_a_file",
            format!("{} is not a regular file", path.display()),
        ))
    } else {
        Ok(())
    }
}

/// Whether `path` is inside git metadata (any component git would read as
/// `.git`). Writes there could plant repository config such as
/// `diff.external` that a later `git_exec` would run, so fs writes and
/// deletes refuse it.
fn is_git_metadata(path: &Path) -> bool {
    path.components()
        .any(|c| is_dot_git(&c.as_os_str().to_string_lossy()))
}

/// `.git` as macOS (case-insensitive APFS/HFS+, which also ignores some
/// zero-width code points) and git itself (`is_hfs_dotgit`) see it.
fn is_dot_git(name: &str) -> bool {
    const IGNORABLE: [char; 16] = [
        '\u{200C}', '\u{200D}', '\u{200E}', '\u{200F}', '\u{202A}', '\u{202B}', '\u{202C}',
        '\u{202D}', '\u{202E}', '\u{206A}', '\u{206B}', '\u{206C}', '\u{206D}', '\u{206E}',
        '\u{206F}', '\u{FEFF}',
    ];
    let visible: String = name.chars().filter(|c| !IGNORABLE.contains(c)).collect();
    visible.eq_ignore_ascii_case(".git")
}

fn strings_of(args: &[&str]) -> Vec<String> {
    args.iter().map(|a| a.to_string()).collect()
}

/// git discovery stops above the innermost root containing `cwd`, so git never
/// uses an enclosing repository (a dotfiles repo at `$HOME`, say).
fn git_ceiling(cwd: &Path, roots: &[PathBuf]) -> Option<PathBuf> {
    roots
        .iter()
        .filter(|root| cwd.starts_with(root))
        .max_by_key(|root| root.components().count())
        .and_then(|root| root.parent().map(Path::to_path_buf))
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
    if is_git_metadata(&resolved) {
        return not_allowed(&resolved);
    }
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
        // Created with the existing file's mode (narrowed by the umask), so the
        // temp file is never more readable than the file it replaces; a new
        // file gets the usual 0666 & !umask.
        let mode = existing.map_or(0o666, |meta| meta.permissions().mode() & 0o7777);
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(mode)
            .open(&tmp)?;
        if let Some(meta) = existing {
            // Restore bits the umask dropped, before any data is written.
            file.set_permissions(meta.permissions())?;
        }
        file.write_all(data)?;
        file.sync_all()?;
        std::fs::rename(&tmp, target)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

fn fs_delete(path: &str, roots: &FsRoots) -> Response {
    let resolved = match resolve_confined(path, &roots.writable, false) {
        Ok(p) => p,
        Err(e) => return e,
    };
    // A root (project, worktree, transcript dir) or any ancestor of one, such
    // as `<project>/.pu/worktrees`, is never deletable here.
    if roots.all.iter().any(|root| root.starts_with(&resolved)) || is_git_metadata(&resolved) {
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

/// A relative path made only of plain names (no `..`, no root).
fn is_plain_relative(path: &str) -> bool {
    let path = Path::new(path);
    !path.as_os_str().is_empty() && path.components().all(|c| matches!(c, Component::Normal(_)))
}

/// Refuses `git`/`gh` arguments that could escape the confined working
/// directory or run arbitrary programs. The app's own invocations
/// (`GitService.swift`) all pass.
fn check_exec_args(program: &str, args: &[String]) -> Result<(), String> {
    match program {
        "git" => check_git_args(args),
        "gh" => check_gh_args(args),
        _ => Err(format!("git_exec runs only git or gh, not {program:?}")),
    }
}

fn check_git_args(args: &[String]) -> Result<(), String> {
    let mut i = 0;
    while let Some(arg) = args.get(i) {
        if !arg.starts_with('-') {
            break;
        }
        if arg == "-c" {
            let setting = args.get(i + 1).map(String::as_str).unwrap_or_default();
            let key = setting.split_once('=').map_or(setting, |(k, _)| k);
            if !GIT_CONFIG_KEYS.contains(&key.to_ascii_lowercase().as_str()) {
                return Err(format!("git -c {setting:?} is not allowed"));
            }
            i += 2;
            continue;
        }
        if !GIT_GLOBAL_FLAGS.contains(&arg.as_str()) {
            return Err(format!("git global option {arg:?} is not allowed"));
        }
        i += 1;
    }
    let Some(subcommand) = args.get(i) else {
        return Err("git_exec needs a git subcommand".into());
    };
    if !GIT_ALLOWED_SUBCOMMANDS.contains(&subcommand.as_str()) {
        return Err(format!("git {subcommand} is not allowed"));
    }
    let rest = &args[i + 1..];
    if subcommand == "symbolic-ref" {
        check_symbolic_ref_read(rest)?;
    }
    let separator = rest.iter().position(|a| a == "--");
    let options = &rest[..separator.unwrap_or(rest.len())];
    // A positive allowlist per subcommand of exactly the options the app sends:
    // any other option (bundled short flags such as `-RO<file>`, `ls-files
    // -X<file>`, `--output`, ...) could read or write outside the roots.
    let mut no_index = false;
    let mut operands_before: Vec<&String> = Vec::new();
    let mut end_of_options = false;
    let mut j = 0;
    while let Some(arg) = options.get(j) {
        j += 1;
        if end_of_options || !arg.starts_with('-') {
            operands_before.push(arg);
            continue;
        }
        if subcommand == "rev-parse" && arg == "--end-of-options" {
            end_of_options = true;
            continue;
        }
        match git_option_rule(subcommand, arg) {
            Some(GitOption::Flag) => {}
            Some(GitOption::Count) => {
                let value = options.get(j).map(String::as_str).unwrap_or_default();
                if value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
                    return Err(format!("git {subcommand} {arg} needs a number"));
                }
                j += 1;
            }
            None => return Err(format!("git {subcommand} option {arg:?} is not allowed")),
        }
        if arg == "--no-index" {
            no_index = true;
        }
    }
    // Paths name files inside the repository. An absolute or `..` path would read
    // outside it (git even switches `diff` to no-index mode on its own for a path
    // outside the work tree), so only `/dev/null` and plain relative paths pass.
    let after_separator = separator.map_or(&rest[rest.len()..], |s| &rest[s + 1..]);
    let operands = operands_before.into_iter().chain(after_separator);
    for arg in operands {
        if arg != "/dev/null" && (arg.starts_with('/') || arg.split('/').any(|c| c == "..")) {
            return Err(format!("git path {arg:?} is outside the repository"));
        }
    }
    if no_index {
        // `diff --no-index` compares arbitrary files; allow only the app's
        // "new file" form: options, `--`, then /dev/null or plain relative paths.
        let paths = separator.map(|s| &rest[s + 1..]);
        let ok = subcommand == "diff"
            && options.iter().all(|a| a.starts_with('-'))
            && paths.is_some_and(|paths| {
                paths.len() == 2
                    && paths
                        .iter()
                        .all(|p| p == "/dev/null" || is_plain_relative(p))
            });
        if !ok {
            return Err(
                "git diff --no-index is allowed only as `-- /dev/null <relative path>`".into(),
            );
        }
    }
    Ok(())
}

/// How an allowed option is used.
enum GitOption {
    Flag,
    /// Takes a separate numeric value (`-n 1`).
    Count,
}

/// Options shared by the app's `diff` and `diff-tree` calls.
const GIT_DIFF_OPTIONS: [&str; 13] = [
    "--no-color",
    "--no-ext-diff",
    "--no-textconv",
    "--find-renames",
    "--src-prefix=a/",
    "--dst-prefix=b/",
    "--name-status",
    "-z",
    "--raw",
    "--no-abbrev",
    "--numstat",
    "--patch",
    "--no-renames",
];

/// The options `subcommand` may take — exactly those `GitService.swift` sends.
fn git_option_rule(subcommand: &str, arg: &str) -> Option<GitOption> {
    let flags: &[&str] = match subcommand {
        "status" => &[
            "--porcelain=v1",
            "-z",
            "--untracked-files=all",
            "--no-renames",
        ],
        "diff" if ["--cached", "--ours", "--no-index"].contains(&arg) => {
            return Some(GitOption::Flag);
        }
        "diff" => &GIT_DIFF_OPTIONS,
        "diff-tree" if ["--root", "--no-commit-id", "-r"].contains(&arg) => {
            return Some(GitOption::Flag);
        }
        "diff-tree" => &GIT_DIFF_OPTIONS,
        // Format placeholders only render commit and ref data.
        "log" | "for-each-ref" if arg.starts_with("--format=") => return Some(GitOption::Flag),
        "rev-parse" => &["--abbrev-ref", "--symbolic-full-name", "--verify"],
        "rev-list" if arg == "-n" => return Some(GitOption::Count),
        "rev-list" => &["--count", "--parents", "--stdin"],
        "ls-files" => &["--others", "--exclude-standard", "-z", "--unmerged"],
        "symbolic-ref" => &["--short", "-q", "--quiet"],
        _ => &[],
    };
    flags.contains(&arg).then_some(GitOption::Flag)
}

/// Path operands that exist must stay inside the roots once symlinks are
/// resolved: `diff --no-index` (explicit, or implicit outside a repository)
/// reads operands straight from the filesystem.
fn check_operand_targets(cwd: &Path, args: &[String], roots: &[PathBuf]) -> Result<(), String> {
    for arg in args
        .iter()
        .filter(|a| !a.starts_with('-') && a.as_str() != "/dev/null")
    {
        let candidate = cwd.join(arg);
        if std::fs::symlink_metadata(&candidate).is_err() {
            continue; // a revision, or a path that doesn't exist
        }
        let inside = candidate
            .canonicalize()
            .is_ok_and(|real| roots.iter().any(|root| real.starts_with(root)));
        if !inside {
            return Err(format!("git path {arg:?} resolves outside the project"));
        }
    }
    Ok(())
}

/// `symbolic-ref` only in its read form, `[--short] [-q] <name>`: a second
/// name would repoint the ref and `-d` would delete it.
fn check_symbolic_ref_read(rest: &[String]) -> Result<(), String> {
    let mut names = 0;
    for arg in rest {
        match arg.as_str() {
            "--short" | "-q" | "--quiet" => {}
            a if a.starts_with('-') => {
                return Err(format!("git symbolic-ref option {a:?} is not allowed"));
            }
            _ => names += 1,
        }
    }
    if names == 1 {
        Ok(())
    } else {
        Err("git symbolic-ref is allowed only to read one ref".into())
    }
}

fn check_gh_args(args: &[String]) -> Result<(), String> {
    let command = args.first().map(String::as_str).unwrap_or_default();
    let subcommand = args.get(1).map(String::as_str).unwrap_or_default();
    let allowed = GH_ALLOWED
        .iter()
        .any(|(c, subs)| *c == command && subs.contains(&subcommand));
    if !allowed {
        return Err(format!("gh {command} {subcommand} is not allowed"));
    }
    // Options are limited to the ones the app sends: `-R/--repo`, `--web` and
    // the like would reach other repositories or open a browser on the host.
    let valued: &[&str] = match (command, subcommand) {
        ("pr", "list") => &["--json", "--limit", "--head"],
        _ => &[],
    };
    let mut rest = args[2..].iter();
    while let Some(arg) = rest.next() {
        if !arg.starts_with('-') {
            continue;
        }
        if !valued.contains(&arg.as_str()) || rest.next().is_none() {
            return Err(format!(
                "gh {command} {subcommand} option {arg:?} is not allowed"
            ));
        }
    }
    Ok(())
}

/// The argv `git_exec` runs for already-checked `args`: git also gets
/// [`GIT_EXEC_PREFIX`].
fn exec_args(program: &str, args: Vec<String>) -> Vec<String> {
    if program != "git" {
        return args;
    }
    let mut out: Vec<String> = GIT_EXEC_PREFIX.iter().map(|a| a.to_string()).collect();
    let mut args = args.into_iter().peekable();
    // Global options (already checked) come before the subcommand.
    while let Some(arg) = args.next_if(|a| a.starts_with('-')) {
        let takes_value = arg == "-c";
        out.push(arg);
        if takes_value && let Some(value) = args.next() {
            out.push(value);
        }
    }
    if let Some(subcommand) = args.next() {
        // Never run an external diff or textconv driver a repository's own
        // config or attributes name.
        let no_drivers = matches!(subcommand.as_str(), "diff" | "diff-tree" | "log");
        out.push(subcommand);
        if no_drivers {
            out.push("--no-ext-diff".into());
            out.push("--no-textconv".into());
        }
    }
    out.extend(args);
    out
}

/// Extra environment for `git_exec`: git ignores the user's global and the
/// system config, so neither can configure helpers (fsmonitor, pager, ...).
fn exec_env(program: &str) -> Vec<(&'static str, &'static str)> {
    if program == "git" {
        vec![
            ("GIT_CONFIG_GLOBAL", "/dev/null"),
            ("GIT_CONFIG_NOSYSTEM", "1"),
        ]
    } else {
        // gh runs git internally; those calls get the same repository guards
        // (its own auth lives in gh's config, not git's, so that config stays).
        vec![(
            "GIT_CONFIG_PARAMETERS",
            "'safe.bareRepository=explicit' 'core.fsmonitor=' 'core.hooksPath=/dev/null'",
        )]
    }
}

/// SIGKILL the process group led by `pid` (git_exec children lead their own).
fn kill_process_group(pid: u32) {
    let Ok(pid) = libc::pid_t::try_from(pid) else {
        return;
    };
    if pid > 0 {
        // SAFETY: plain signal delivery to a group this daemon created.
        unsafe {
            libc::kill(-pid, libc::SIGKILL);
        }
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
    async fn fs_roots(&self) -> FsRoots {
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
            let dir = resolve_confined(&path, &roots.all, true)?;
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
                    .handle_git_exec(
                        &cwd,
                        &program,
                        args,
                        stdin_base64,
                        timeout_ms.min(GIT_EXEC_MAX_TIMEOUT_MS),
                    )
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
            } => fs_list(
                &path,
                &roots.all,
                recursive,
                include_hidden,
                max_entries.min(FS_LIST_MAX_ENTRIES),
            ),
            Request::FsStat { path } => fs_stat(&path, &roots.all),
            Request::FsRead { path, max_bytes } => fs_read(&path, &roots.all, max_bytes),
            Request::FsWrite {
                path,
                data_base64,
                expected_modified_ms,
                create_dirs,
            } => fs_write(
                &path,
                &roots.writable,
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
        if let Err(message) = check_exec_args(program, &args) {
            return fs_error("args_not_allowed", message);
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
        // git must not walk up out of the confined root into an enclosing
        // repository (a dotfiles repo at `$HOME`, say): stop discovery at the
        // innermost root containing `cwd`.
        let roots = self.fs_roots().await;
        let ceiling = git_ceiling(&cwd, &roots.all);
        if program == "git"
            && let Err(message) = check_operand_targets(&cwd, &args, &roots.all)
        {
            return fs_error("args_not_allowed", message);
        }
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
            .args(exec_args(program, args))
            .current_dir(&cwd)
            .envs(exec_env(program))
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GH_PROMPT_DISABLED", "1")
            .stdin(if stdin_data.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            // Own process group, so a timeout also stops helpers git or gh
            // spawned (pagers, credential helpers, hooks).
            .process_group(0)
            .kill_on_drop(true);
        if let Some(path) = login_path {
            command.env("PATH", path);
        }
        if let Some(ceiling) = ceiling {
            command.env("GIT_CEILING_DIRECTORIES", ceiling);
        }
        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(e) => return fs_error("io_error", format!("failed to run {program}: {e}")),
        };
        let pgid = child.id();
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
                if let Some(pgid) = pgid {
                    kill_process_group(pgid);
                }
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
