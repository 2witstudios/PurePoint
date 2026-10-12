//! macOS process introspection and parent-death watch for the guardian.
//!
//! Process table: `proc_listallpids` + `proc_pidinfo(PROC_PIDTBSDINFO)`.
//! Environment: `sysctl(KERN_PROCARGS2)`, readable only for same-uid
//! processes. Start time is microseconds since the epoch from
//! `pbi_start_tv{sec,usec}`, stable for the life of a pid (survives exec).
//! Parent death: kqueue `EVFILT_PROC`/`NOTE_EXIT` on the expected parent pid.

use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};

use super::supervisor::ProcEntry;

pub(crate) const MECHANISM: &str = "guardian-v1-macos";

fn bsdinfo(pid: i32) -> Option<libc::proc_bsdinfo> {
    // SAFETY: proc_bsdinfo is plain old data; zeroed is a valid value.
    let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<libc::proc_bsdinfo>() as libc::c_int;
    // SAFETY: the buffer is a live, writable proc_bsdinfo of `size` bytes.
    let n = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDTBSDINFO,
            0,
            (&mut info as *mut libc::proc_bsdinfo).cast(),
            size,
        )
    };
    (n == size).then_some(info)
}

fn entry(info: &libc::proc_bsdinfo) -> ProcEntry {
    ProcEntry {
        pid: info.pbi_pid as i32,
        ppid: info.pbi_ppid as i32,
        uid: info.pbi_uid,
        start: info
            .pbi_start_tvsec
            .saturating_mul(1_000_000)
            .saturating_add(info.pbi_start_tvusec),
        zombie: info.pbi_status == libc::SZOMB,
    }
}

pub(crate) fn process(pid: i32) -> Option<ProcEntry> {
    bsdinfo(pid).map(|i| entry(&i)).filter(|e| e.pid == pid)
}

pub(crate) fn list_processes() -> Vec<ProcEntry> {
    let mut pids: Vec<libc::pid_t> = vec![0; 4096];
    loop {
        let bytes = (pids.len() * std::mem::size_of::<libc::pid_t>()) as libc::c_int;
        // SAFETY: the buffer holds `bytes` writable bytes of pid_t storage.
        let n = unsafe { libc::proc_listallpids(pids.as_mut_ptr().cast(), bytes) };
        if n < 0 {
            return Vec::new();
        }
        let n = n as usize;
        if n < pids.len() {
            pids.truncate(n);
            break;
        }
        pids.resize(pids.len() * 2, 0);
    }
    pids.into_iter()
        .filter(|&p| p > 0)
        .filter_map(process)
        .collect()
}

/// Raw NUL-separated environment block of `pid`, or None when unreadable.
pub(crate) fn environment(pid: i32, buf: &mut Vec<u8>) -> Option<Vec<u8>> {
    if buf.is_empty() {
        let mut argmax: libc::c_int = 0;
        let mut size = std::mem::size_of::<libc::c_int>();
        let mut mib = [libc::CTL_KERN, libc::KERN_ARGMAX];
        // SAFETY: mib/argmax/size point to live storage of the declared sizes.
        let rc = unsafe {
            libc::sysctl(
                mib.as_mut_ptr(),
                2,
                (&mut argmax as *mut libc::c_int).cast(),
                &mut size,
                std::ptr::null_mut(),
                0,
            )
        };
        let argmax = if rc == 0 && argmax > 0 {
            argmax as usize
        } else {
            1 << 20
        };
        buf.resize(argmax, 0);
    }
    let mut mib = [libc::CTL_KERN, libc::KERN_PROCARGS2, pid];
    let mut size = buf.len();
    // SAFETY: buf has `size` writable bytes; mib is a live 3-int array.
    let rc = unsafe {
        libc::sysctl(
            mib.as_mut_ptr(),
            3,
            buf.as_mut_ptr().cast(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if rc != 0 || size < 4 {
        return None;
    }
    parse_procargs2(&buf[..size])
}

/// Layout: argc (i32), exec path, NUL padding, argc argv strings, then env
/// strings up to an empty string.
pub(crate) fn parse_procargs2(data: &[u8]) -> Option<Vec<u8>> {
    let argc = i32::from_ne_bytes(data.get(..4)?.try_into().ok()?);
    let mut i = 4;
    while *data.get(i)? != 0 {
        i += 1;
    }
    while i < data.len() && data[i] == 0 {
        i += 1;
    }
    for _ in 0..argc.max(0) {
        while *data.get(i)? != 0 {
            i += 1;
        }
        i += 1;
    }
    let start = i;
    // Env strings end at the first empty string (two consecutive NULs).
    let mut end = start;
    while end < data.len() {
        let len = data[end..].iter().position(|&b| b == 0)?;
        if len == 0 {
            break;
        }
        end += len + 1;
    }
    Some(data[start..end].to_vec())
}

/// kqueue watch on the expected parent's exit.
pub(crate) struct ParentWatch {
    kq: OwnedFd,
    gone: bool,
}

impl ParentWatch {
    /// Register NOTE_EXIT on `parent`; ESRCH means it is already gone.
    pub(crate) fn arm(parent: i32) -> io::Result<Self> {
        // SAFETY: kqueue has no arguments; the result is checked.
        let raw = unsafe { libc::kqueue() };
        if raw < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: raw is a fresh descriptor this function owns.
        let kq = unsafe { OwnedFd::from_raw_fd(raw) };
        // SAFETY: setting FD_CLOEXEC on an owned descriptor.
        unsafe { libc::fcntl(raw, libc::F_SETFD, libc::FD_CLOEXEC) };
        // SAFETY: kevent is plain old data; zeroed is valid before filling.
        let mut change: libc::kevent = unsafe { std::mem::zeroed() };
        change.ident = parent as libc::uintptr_t;
        change.filter = libc::EVFILT_PROC;
        change.flags = libc::EV_ADD | libc::EV_ENABLE | libc::EV_ONESHOT;
        change.fflags = libc::NOTE_EXIT;
        // SAFETY: one live changelist entry, no event list.
        let rc =
            unsafe { libc::kevent(raw, &change, 1, std::ptr::null_mut(), 0, std::ptr::null()) };
        let gone = if rc < 0 {
            let err = io::Error::last_os_error();
            if err.raw_os_error() != Some(libc::ESRCH) {
                return Err(err);
            }
            true
        } else {
            false
        };
        Ok(Self { kq, gone })
    }

    pub(crate) fn poll_fd(&self) -> Option<RawFd> {
        Some(self.kq.as_raw_fd())
    }

    /// Non-blocking: has the parent exit event fired?
    pub(crate) fn parent_exited(&mut self) -> bool {
        if self.gone {
            return true;
        }
        // SAFETY: kevent is plain old data; zero timeout makes this a poll.
        let mut out: libc::kevent = unsafe { std::mem::zeroed() };
        let zero = libc::timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
        // SAFETY: one writable event slot; no changes; zero timeout.
        let n =
            unsafe { libc::kevent(self.kq.as_raw_fd(), std::ptr::null(), 0, &mut out, 1, &zero) };
        if n > 0 && out.filter == libc::EVFILT_PROC && out.fflags & libc::NOTE_EXIT != 0 {
            self.gone = true;
        }
        self.gone
    }
}

/// No subreaper on macOS: orphans reparent to launchd.
pub(crate) fn become_subreaper() {}

/// `struct proc_fileinfo` from <sys/proc_info.h> (not exported by libc).
#[repr(C)]
struct ProcFileInfo {
    fi_openflags: u32,
    fi_status: u32,
    fi_offset: libc::off_t,
    fi_type: i32,
    fi_guardflags: u32,
}

/// `struct vnode_fdinfo` from <sys/proc_info.h>.
#[repr(C)]
struct VnodeFdInfo {
    pfi: ProcFileInfo,
    pvi: libc::vnode_info,
}

const PROC_PIDFDVNODEINFO: libc::c_int = 1;

/// True when `pid` holds an open descriptor on the vnode `(dev, ino)`.
/// Works for same-uid processes, including Apple platform binaries whose
/// environment `KERN_PROCARGS2` no longer exposes.
pub(crate) fn holds_vnode(pid: i32, (dev, ino): (u64, u64)) -> bool {
    let entry = std::mem::size_of::<libc::proc_fdinfo>();
    let mut fds: Vec<libc::proc_fdinfo> = Vec::with_capacity(256);
    loop {
        let cap = fds.capacity();
        // SAFETY: the buffer has cap*entry writable bytes of proc_fdinfo storage.
        let n = unsafe {
            libc::proc_pidinfo(
                pid,
                libc::PROC_PIDLISTFDS,
                0,
                fds.as_mut_ptr().cast(),
                (cap * entry) as libc::c_int,
            )
        };
        if n <= 0 {
            return false;
        }
        let count = (n as usize / entry).min(cap);
        if count < cap || cap >= 1 << 16 {
            // SAFETY: the kernel initialized `count` entries.
            unsafe { fds.set_len(count) };
            break;
        }
        fds.reserve(cap * 4);
    }
    fds.iter()
        .filter(|f| f.proc_fdtype == libc::PROX_FDTYPE_VNODE as u32)
        .any(|f| {
            // SAFETY: VnodeFdInfo is plain old data; zeroed is valid.
            let mut info: VnodeFdInfo = unsafe { std::mem::zeroed() };
            let size = std::mem::size_of::<VnodeFdInfo>() as libc::c_int;
            // SAFETY: info is a live writable VnodeFdInfo of `size` bytes.
            let n = unsafe {
                libc::proc_pidfdinfo(
                    pid,
                    f.proc_fd,
                    PROC_PIDFDVNODEINFO,
                    (&mut info as *mut VnodeFdInfo).cast(),
                    size,
                )
            };
            n == size && info.pvi.vi_stat.vst_dev as u64 == dev && info.pvi.vi_stat.vst_ino == ino
        })
}
