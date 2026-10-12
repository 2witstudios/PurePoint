//! Linux process introspection and parent-death watch for the guardian.
//!
//! Process table and start time (clock ticks since boot, field 22) from
//! `/proc/<pid>/stat`; real uid from the `Uid:` line of `/proc/<pid>/status`
//! (not the owner of `/proc/<pid>`, which is root for non-dumpable processes
//! such as ssh-agent, so they would otherwise look foreign); environment from
//! `/proc/<pid>/environ` (mode 0400, same uid only). Parent death:
//! `PR_SET_PDEATHSIG` plus a `getppid()` re-check; `PR_SET_CHILD_SUBREAPER`
//! keeps orphaned descendants parented to the guardian.
//!
//! NOT RUN on this branch's evidence host (macOS); compiled only under
//! `cfg(target_os = "linux")`.

use std::io;
use std::os::fd::RawFd;
use std::os::unix::fs::MetadataExt;

use super::supervisor::ProcEntry;

pub(crate) const MECHANISM: &str = "guardian-v1-linux";

/// Parent-death signal. Not SIGTERM: PDEATHSIG fires when the *thread* that
/// forked us exits, which in a multi-threaded daemon is not daemon death. A
/// dedicated signal only wakes the guardian, which then trusts `getppid()`.
pub(crate) const PDEATH_SIGNAL: libc::c_int = libc::SIGUSR2;

pub(crate) fn process(pid: i32) -> Option<ProcEntry> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let uid = real_uid(pid)?;
    let rest = &stat[stat.rfind(')')? + 2..];
    let fields: Vec<&str> = rest.split_whitespace().collect();
    // rest[0] is field 3 (state); ppid is field 4; starttime is field 22.
    Some(ProcEntry {
        pid,
        ppid: fields.get(1)?.parse().ok()?,
        uid,
        start: fields.get(19)?.parse().ok()?,
        zombie: matches!(*fields.first()?, "Z" | "X"),
    })
}

/// Real uid of `pid`. `/proc/<pid>/status` stays world-readable even when the
/// process is non-dumpable.
fn real_uid(pid: i32) -> Option<u32> {
    let status = std::fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    parse_real_uid(&status)
}

fn parse_real_uid(status: &str) -> Option<u32> {
    status
        .lines()
        .find_map(|line| line.strip_prefix("Uid:"))?
        .split_whitespace()
        .next()?
        .parse()
        .ok()
}

pub(crate) fn list_processes() -> Vec<ProcEntry> {
    let Ok(dir) = std::fs::read_dir("/proc") else {
        return Vec::new();
    };
    dir.filter_map(|e| e.ok()?.file_name().to_str()?.parse::<i32>().ok())
        .filter_map(process)
        .collect()
}

// Signature shared with the macOS sysctl reader, which grows the buffer.
#[allow(clippy::ptr_arg)]
pub(crate) fn environment(pid: i32, _buf: &mut Vec<u8>) -> Option<Vec<u8>> {
    std::fs::read(format!("/proc/{pid}/environ")).ok()
}

pub(crate) struct ParentWatch {
    gone: bool,
}

impl ParentWatch {
    pub(crate) fn arm(parent: i32) -> io::Result<Self> {
        // SAFETY: prctl with integer arguments only.
        if unsafe { libc::prctl(libc::PR_SET_PDEATHSIG, PDEATH_SIGNAL as libc::c_ulong) } != 0 {
            return Err(io::Error::last_os_error());
        }
        // Close the race: the parent may have died before PDEATHSIG was set.
        // SAFETY: getppid has no preconditions.
        let gone = unsafe { libc::getppid() } != parent;
        Ok(Self { gone })
    }

    pub(crate) fn poll_fd(&self) -> Option<RawFd> {
        None
    }

    pub(crate) fn parent_exited(&mut self) -> bool {
        self.gone
    }
}

/// True when `pid` holds an open descriptor on the file `(dev, ino)`.
pub(crate) fn holds_vnode(pid: i32, (dev, ino): (u64, u64)) -> bool {
    let Ok(dir) = std::fs::read_dir(format!("/proc/{pid}/fd")) else {
        return false;
    };
    dir.filter_map(|e| std::fs::metadata(e.ok()?.path()).ok())
        .any(|m| m.dev() == dev && m.ino() == ino)
}

pub(crate) fn become_subreaper() {
    // SAFETY: prctl with integer arguments only. Best effort.
    unsafe {
        libc::prctl(libc::PR_SET_CHILD_SUBREAPER, 1 as libc::c_ulong);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_status_of_non_dumpable_process_should_read_real_uid_not_proc_owner() {
        // given: /proc/<pid> may be owned by root, but status names the real uid
        let status = "Name:\tssh-agent\nUmask:\t0077\nUid:\t1000\t1000\t1000\t1000\nGid:\t1000\t1000\t1000\t1000\n";
        // when / then
        assert_eq!(parse_real_uid(status), Some(1000));
        assert_eq!(parse_real_uid("Name:\tx\n"), None);
    }

    #[test]
    fn given_current_process_should_read_its_own_real_uid() {
        // SAFETY: getuid has no preconditions.
        let uid = unsafe { libc::getuid() };
        assert_eq!(real_uid(std::process::id() as i32), Some(uid));
    }
}
