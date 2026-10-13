//! Host facts the daemon needs that differ between macOS and Linux servers.
//!
//! On Linux these read `/proc` and the passwd database directly: minimal server
//! and container images often lack `lsof`, ship a `ps` without BSD flags, and
//! run the daemon under systemd with `SHELL` unset.

use std::collections::HashMap;
use std::path::PathBuf;

/// The user's login shell: `$SHELL`, then the passwd entry, then `/bin/sh`.
/// Never a shell that might not be installed (e.g. zsh on a Linux server).
pub fn login_shell() -> String {
    std::env::var("SHELL")
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(passwd_shell)
        .unwrap_or_else(|| "/bin/sh".into())
}

fn passwd_shell() -> Option<String> {
    let user = nix::unistd::User::from_uid(nix::unistd::getuid()).ok()??;
    let shell = user.shell.to_string_lossy().into_owned();
    (!shell.is_empty() && user.shell.exists()).then_some(shell)
}

/// Account name for `uid` from the passwd database.
pub fn user_name(uid: u32) -> Option<String> {
    let user = nix::unistd::User::from_uid(nix::unistd::Uid::from_raw(uid)).ok()??;
    (!user.name.is_empty()).then_some(user.name)
}

/// Parse `/proc/<pid>/stat` into `(pid, ppid)`. The command name (field 2) is
/// parenthesised and may itself contain spaces or `)`, so fields are read after
/// the last `)`: state, then ppid.
pub fn parse_proc_stat(stat: &str) -> Option<(i32, i32)> {
    let pid = stat.split_whitespace().next()?.parse().ok()?;
    let rest = &stat[stat.rfind(')')? + 1..];
    let mut fields = rest.split_whitespace();
    let _state = fields.next()?;
    let ppid = fields.next()?.parse().ok()?;
    Some((pid, ppid))
}

/// Parent→children map of every process, from `/proc/*/stat`.
#[cfg(target_os = "linux")]
pub fn proc_process_tree() -> Option<HashMap<i32, Vec<i32>>> {
    let mut children: HashMap<i32, Vec<i32>> = HashMap::new();
    for entry in std::fs::read_dir("/proc").ok()?.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if !name.bytes().all(|b| b.is_ascii_digit()) {
            continue;
        }
        // Processes exit between readdir and read; skip them.
        let Ok(stat) = std::fs::read_to_string(entry.path().join("stat")) else {
            continue;
        };
        if let Some((pid, ppid)) = parse_proc_stat(&stat)
            && pid > 0
        {
            children.entry(ppid).or_default().push(pid);
        }
    }
    Some(children)
}

#[cfg(not(target_os = "linux"))]
pub fn proc_process_tree() -> Option<HashMap<i32, Vec<i32>>> {
    None
}

/// `/proc/<pid>/cmdline` with NUL separators as spaces, like `ps -o command=`.
pub fn cmdline_to_command(cmdline: &[u8]) -> String {
    String::from_utf8_lossy(cmdline)
        .trim_end_matches('\0')
        .replace('\0', " ")
}

/// Full command line of `pid`, or None if it is gone or unreadable.
pub fn process_command(pid: u32) -> Option<String> {
    if cfg!(target_os = "linux") {
        return std::fs::read(format!("/proc/{pid}/cmdline"))
            .ok()
            .map(|bytes| cmdline_to_command(&bytes));
    }
    let out = std::process::Command::new("ps")
        .args(["-o", "command=", "-p", &pid.to_string()])
        .stdin(std::process::Stdio::null())
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Paths of files the given processes hold open, from `/proc/<pid>/fd`.
/// Unreadable processes (gone, or another user's) contribute nothing.
#[cfg(target_os = "linux")]
pub fn proc_open_files(pids: &[i32], limit: usize) -> Option<Vec<PathBuf>> {
    let mut paths = Vec::new();
    for pid in pids {
        let Ok(fds) = std::fs::read_dir(format!("/proc/{pid}/fd")) else {
            continue;
        };
        for fd in fds.flatten() {
            if let Ok(target) = std::fs::read_link(fd.path())
                && target.is_absolute()
            {
                paths.push(target);
                if paths.len() >= limit {
                    return Some(paths);
                }
            }
        }
    }
    Some(paths)
}

#[cfg(not(target_os = "linux"))]
pub fn proc_open_files(_pids: &[i32], _limit: usize) -> Option<Vec<PathBuf>> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_comm_with_spaces_and_parens_should_parse_ppid() {
        assert_eq!(
            parse_proc_stat("4242 (my (odd) cmd) S 17 4242 4242 0 -1"),
            Some((4242, 17))
        );
        assert_eq!(parse_proc_stat("1 (systemd) S 0 1 1"), Some((1, 0)));
        assert_eq!(parse_proc_stat("garbage"), None);
        assert_eq!(parse_proc_stat(""), None);
    }

    #[test]
    fn given_nul_separated_cmdline_should_join_with_spaces() {
        assert_eq!(
            cmdline_to_command(b"codex\0resume\0abc-123\0"),
            "codex resume abc-123"
        );
        assert_eq!(cmdline_to_command(b""), "");
    }

    #[test]
    fn given_login_shell_should_be_nonempty_absolute_path() {
        let shell = login_shell();
        assert!(shell.starts_with('/'), "{shell}");
    }

    #[test]
    fn given_current_uid_should_resolve_user_name() {
        // SAFETY: geteuid has no preconditions.
        let uid = unsafe { libc::geteuid() };
        assert!(user_name(uid).is_some_and(|n| !n.is_empty()));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn given_spawned_child_should_appear_in_proc_tree() {
        let mut child = std::process::Command::new("sleep")
            .arg("5")
            .spawn()
            .unwrap();
        let tree = proc_process_tree().unwrap();
        let ours = std::process::id() as i32;
        let found = tree
            .get(&ours)
            .is_some_and(|kids| kids.contains(&(child.id() as i32)));
        child.kill().ok();
        child.wait().ok();
        assert!(found, "child missing from /proc tree");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn given_open_file_should_appear_in_proc_fds() {
        let tmp = tempfile::TempDir::new().unwrap();
        let path = tmp.path().join("rollout-1.jsonl");
        let _file = std::fs::File::create(&path).unwrap();
        let canonical = std::fs::canonicalize(&path).unwrap();
        let open = proc_open_files(&[std::process::id() as i32], 100_000).unwrap();
        assert!(open.contains(&canonical), "{open:?}");
    }
}
