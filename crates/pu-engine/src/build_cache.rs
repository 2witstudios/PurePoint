//! Cargo build caches created by agents.
//!
//! Worktree agents build into `<worktree>/target`: the daemon sets
//! `CARGO_TARGET_DIR` when it spawns them, and deleting the worktree deletes
//! the cache. Agents used to invent `/tmp/purepoint-<slice>-target` dirs that
//! nothing removed; the sweeper deletes those once idle and, when the disk is
//! low, also drops idle caches of worktrees that still exist.

use std::fs::File;
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

pub const CARGO_TARGET_DIR_ENV: &str = "CARGO_TARGET_DIR";

/// How often the daemon sweeps for abandoned caches.
pub const SWEEP_INTERVAL: Duration = Duration::from_secs(30 * 60);
/// A cache nobody has built into for this long is abandoned.
pub const IDLE_AFTER: Duration = Duration::from_secs(6 * 3600);
/// Below this much free disk, sweep harder.
pub const LOW_DISK_FREE_BYTES: u64 = 20 * 1024 * 1024 * 1024;
pub const LOW_DISK_IDLE_AFTER: Duration = Duration::from_secs(30 * 60);

const LEGACY_PREFIX: &str = "purepoint-";
const LEGACY_SUFFIX: &str = "-target";
/// Cargo holds an flock on these for the whole command, in each
/// `<profile>/` or `<triple>/<profile>/` dir.
const CARGO_LOCK_FILES: [&str; 2] = [".cargo-lock", ".cargo-build-lock"];
/// Deep enough to reach `<triple>/<profile>/deps` and `.fingerprint/<pkg>`.
const SCAN_DEPTH: usize = 3;

pub fn worktree_target_dir(worktree: &Path) -> PathBuf {
    worktree.join("target")
}

/// Point a worktree agent's Cargo builds at its own worktree, replacing any
/// target dir inherited from the login shell. Root agents keep the project's.
pub fn apply_cargo_target_env(env: &mut Vec<(String, String)>, worktree: Option<&Path>) {
    let Some(worktree) = worktree else { return };
    env.retain(|(k, _)| k != CARGO_TARGET_DIR_ENV);
    env.push((
        CARGO_TARGET_DIR_ENV.to_string(),
        worktree_target_dir(worktree).to_string_lossy().to_string(),
    ));
}

/// `/tmp/purepoint-<slice>-target` dirs from the old per-agent convention.
/// Symlinks are skipped so a sweep never follows one out of the temp dir.
pub fn legacy_target_dirs(tmp_root: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(tmp_root) else {
        return Vec::new();
    };
    let mut dirs: Vec<PathBuf> = entries
        .flatten()
        .filter(|e| {
            let name = e.file_name();
            let name = name.to_string_lossy();
            name.starts_with(LEGACY_PREFIX)
                && name.ends_with(LEGACY_SUFFIX)
                && e.file_type().is_ok_and(|t| t.is_dir())
        })
        .map(|e| e.path())
        .collect();
    dirs.sort();
    dirs
}

/// `<project>/.pu/worktrees/*/target` caches that exist on disk.
pub fn worktree_target_dirs(project_root: &Path) -> Vec<PathBuf> {
    let worktrees = pu_core::paths::pu_dir(project_root).join("worktrees");
    let Ok(entries) = std::fs::read_dir(worktrees) else {
        return Vec::new();
    };
    let mut dirs: Vec<PathBuf> = entries
        .flatten()
        .map(|e| worktree_target_dir(&e.path()))
        .filter(|t| std::fs::symlink_metadata(t).is_ok_and(|m| m.is_dir()))
        .collect();
    dirs.sort();
    dirs
}

/// Temp roots agents write caches into: the literal `/tmp` agents type, and
/// the per-user `$TMPDIR` on macOS.
pub fn tmp_roots() -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = [PathBuf::from("/tmp"), std::env::temp_dir()]
        .into_iter()
        .filter_map(|p| p.canonicalize().ok())
        .collect();
    roots.sort();
    roots.dedup();
    roots
}

/// True when `dir` is a Cargo cache that no build is using and nothing has
/// been built into for `idle_after`.
pub fn is_idle(dir: &Path, now: SystemTime, idle_after: Duration) -> bool {
    if !is_cargo_target_dir(dir) {
        return false;
    }
    let mut newest = None;
    let mut locked = false;
    scan(dir, SCAN_DEPTH, &mut |path, meta| {
        if let Ok(modified) = meta.modified() {
            newest = newest.max(Some(modified));
        }
        let is_lock = path
            .file_name()
            .is_some_and(|n| CARGO_LOCK_FILES.iter().any(|l| n == *l));
        if is_lock && meta.is_file() && lock_is_held(path) {
            locked = true;
        }
    });
    let Some(newest) = newest else { return false };
    !locked
        && now
            .duration_since(newest)
            .is_ok_and(|age| age >= idle_after)
}

pub struct SweepPolicy {
    pub idle_after: Duration,
    /// Also reclaim idle caches of worktrees that still exist.
    pub include_worktree_caches: bool,
}

pub fn sweep_policy(free_bytes: Option<u64>) -> SweepPolicy {
    if free_bytes.is_some_and(|free| free < LOW_DISK_FREE_BYTES) {
        SweepPolicy {
            idle_after: LOW_DISK_IDLE_AFTER,
            include_worktree_caches: true,
        }
    } else {
        SweepPolicy {
            idle_after: IDLE_AFTER,
            include_worktree_caches: false,
        }
    }
}

pub fn free_bytes(path: &Path) -> Option<u64> {
    let stat = nix::sys::statvfs::statvfs(path).ok()?;
    #[allow(clippy::useless_conversion)]
    Some(u64::from(stat.blocks_available()) * u64::from(stat.fragment_size()))
}

/// Delete each idle cache in `dirs`, returning the ones removed.
pub fn remove_idle(dirs: &[PathBuf], now: SystemTime, idle_after: Duration) -> Vec<PathBuf> {
    dirs.iter()
        .filter(|dir| is_idle(dir, now, idle_after))
        .filter(|dir| match std::fs::remove_dir_all(dir) {
            Ok(()) => true,
            Err(e) => {
                tracing::warn!(dir = %dir.display(), "failed to remove build cache: {e}");
                false
            }
        })
        .cloned()
        .collect()
}

fn is_cargo_target_dir(dir: &Path) -> bool {
    std::fs::symlink_metadata(dir).is_ok_and(|m| m.is_dir())
        && (dir.join("CACHEDIR.TAG").is_file() || dir.join(".rustc_info.json").is_file())
}

fn scan(dir: &Path, depth: usize, visit: &mut impl FnMut(&Path, &std::fs::Metadata)) {
    if let Ok(meta) = std::fs::symlink_metadata(dir) {
        visit(dir, &meta);
    }
    if depth == 0 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        match entry.file_type() {
            Ok(t) if t.is_dir() => scan(&path, depth - 1, visit),
            Ok(_) => {
                if let Ok(meta) = std::fs::symlink_metadata(&path) {
                    visit(&path, &meta);
                }
            }
            Err(_) => {}
        }
    }
}

fn lock_is_held(path: &Path) -> bool {
    let Ok(file) = File::open(path) else {
        return false;
    };
    // SAFETY: flock on a valid fd owned by `file`; the lock is released when
    // `file` is dropped.
    let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    rc != 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    const HOUR: Duration = Duration::from_secs(3600);

    fn make_target_dir(dir: &Path) {
        std::fs::create_dir_all(dir.join("debug/deps")).unwrap();
        std::fs::write(
            dir.join("CACHEDIR.TAG"),
            b"Signature: 8a477f597d28d172789f06886806bc55",
        )
        .unwrap();
        std::fs::write(dir.join("debug/.cargo-lock"), b"").unwrap();
        std::fs::write(dir.join("debug/deps/libfoo.rlib"), b"x").unwrap();
    }

    fn later(by: Duration) -> SystemTime {
        SystemTime::now() + by
    }

    #[test]
    fn given_worktree_agent_should_point_cargo_at_worktree_target() {
        // given
        let mut env = vec![
            ("PATH".to_string(), "/bin".to_string()),
            (
                CARGO_TARGET_DIR_ENV.to_string(),
                "/shared/target".to_string(),
            ),
        ];

        // when
        apply_cargo_target_env(&mut env, Some(Path::new("/p/.pu/worktrees/wt-1")));

        // then
        let targets: Vec<_> = env
            .iter()
            .filter(|(k, _)| k == CARGO_TARGET_DIR_ENV)
            .collect();
        assert_eq!(
            targets,
            vec![&(
                CARGO_TARGET_DIR_ENV.to_string(),
                "/p/.pu/worktrees/wt-1/target".to_string()
            )]
        );
    }

    #[test]
    fn given_root_agent_should_leave_cargo_target_untouched() {
        // given
        let mut env = vec![(CARGO_TARGET_DIR_ENV.to_string(), "/mine".to_string())];

        // when
        apply_cargo_target_env(&mut env, None);

        // then
        assert_eq!(
            env,
            vec![(CARGO_TARGET_DIR_ENV.to_string(), "/mine".to_string())]
        );
    }

    #[test]
    fn given_tmp_root_should_list_only_agent_target_dirs() {
        // given
        let tmp = TempDir::new().unwrap();
        make_target_dir(&tmp.path().join("purepoint-auth-target"));
        make_target_dir(&tmp.path().join("other-target"));
        make_target_dir(&tmp.path().join("purepoint-notes"));
        std::fs::write(tmp.path().join("purepoint-file-target"), b"").unwrap();
        std::os::unix::fs::symlink(
            tmp.path().join("purepoint-auth-target"),
            tmp.path().join("purepoint-link-target"),
        )
        .unwrap();

        // when
        let found = legacy_target_dirs(tmp.path());

        // then
        assert_eq!(found, vec![tmp.path().join("purepoint-auth-target")]);
    }

    #[test]
    fn given_cache_untouched_past_threshold_should_be_idle() {
        // given
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join("purepoint-x-target");
        make_target_dir(&dir);

        // when
        let idle = is_idle(&dir, later(7 * HOUR), 6 * HOUR);

        // then
        assert!(idle);
    }

    #[test]
    fn given_recently_built_cache_should_not_be_idle() {
        // given
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join("purepoint-x-target");
        make_target_dir(&dir);

        // when
        let idle = is_idle(&dir, later(HOUR), 6 * HOUR);

        // then
        assert!(!idle);
    }

    #[test]
    fn given_build_holding_cargo_lock_should_not_be_idle() {
        // given — cargo holds an flock on <profile>/.cargo-lock while it runs
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join("purepoint-x-target");
        make_target_dir(&dir);
        let lock = File::open(dir.join("debug/.cargo-lock")).unwrap();
        assert_eq!(unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX) }, 0);

        // when
        let idle = is_idle(&dir, later(7 * HOUR), 6 * HOUR);

        // then
        assert!(!idle);
    }

    #[test]
    fn given_dir_that_is_not_a_cargo_cache_should_not_be_idle() {
        // given
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join("purepoint-x-target");
        std::fs::create_dir_all(dir.join("notes")).unwrap();

        // when
        let idle = is_idle(&dir, later(7 * HOUR), 6 * HOUR);

        // then
        assert!(!idle);
    }

    #[test]
    fn given_low_disk_should_sweep_sooner_and_include_worktree_caches() {
        // when
        let policy = sweep_policy(Some(LOW_DISK_FREE_BYTES - 1));

        // then
        assert_eq!(policy.idle_after, LOW_DISK_IDLE_AFTER);
        assert!(policy.include_worktree_caches);
    }

    #[test]
    fn given_ample_disk_should_spare_worktree_caches() {
        // when
        let policy = sweep_policy(Some(LOW_DISK_FREE_BYTES * 2));

        // then
        assert_eq!(policy.idle_after, IDLE_AFTER);
        assert!(!policy.include_worktree_caches);
    }

    #[test]
    fn given_project_worktrees_should_list_their_caches() {
        // given
        let tmp = TempDir::new().unwrap();
        let wt_with_cache = pu_core::paths::worktree_path(tmp.path(), "wt-a");
        make_target_dir(&worktree_target_dir(&wt_with_cache));
        std::fs::create_dir_all(pu_core::paths::worktree_path(tmp.path(), "wt-b")).unwrap();

        // when
        let found = worktree_target_dirs(tmp.path());

        // then
        assert_eq!(found, vec![worktree_target_dir(&wt_with_cache)]);
    }

    #[test]
    fn given_idle_and_active_caches_should_remove_only_idle_ones() {
        // given
        let tmp = TempDir::new().unwrap();
        let idle = tmp.path().join("purepoint-idle-target");
        let busy = tmp.path().join("purepoint-busy-target");
        make_target_dir(&idle);
        make_target_dir(&busy);
        let lock = File::open(busy.join("debug/.cargo-lock")).unwrap();
        assert_eq!(unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX) }, 0);

        // when
        let removed = remove_idle(&[idle.clone(), busy.clone()], later(7 * HOUR), 6 * HOUR);

        // then
        assert_eq!(removed, vec![idle.clone()]);
        assert!(!idle.exists());
        assert!(busy.exists());
    }
}
