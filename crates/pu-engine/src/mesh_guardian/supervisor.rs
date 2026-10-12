//! Synchronous guardian runtime: fork the agent, watch for a trigger, track
//! the run's process tree, and terminate it. Single-threaded by design.

use std::collections::{HashMap, HashSet};
use std::os::unix::process::CommandExt;
use std::process::Command;
use std::sync::atomic::{AtomicI32, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use super::platform;
use super::{GuardianArgs, RUN_TOKEN_ENV, Trigger, chain_reaches, env_block_has_entry};

/// How often the run's process tree is scanned while it is alive.
pub(crate) const SCAN_INTERVAL: Duration = Duration::from_millis(250);
/// Bound on SIGKILL/re-scan after the grace period.
const KILL_DEADLINE: Duration = Duration::from_secs(5);
/// Window after SIGHUP to tell PTY hangup-from-daemon-death from a plain HUP.
const HANGUP_SETTLE: Duration = Duration::from_millis(150);
const TERMINATION_TICK: Duration = Duration::from_millis(20);

#[derive(Clone, Copy, Debug)]
pub(crate) struct ProcEntry {
    pub pid: i32,
    pub ppid: i32,
    pub uid: u32,
    /// Platform start time; (pid, start) identifies one process lifetime.
    pub start: u64,
    pub zombie: bool,
}

// ---- signals --------------------------------------------------------------

static WAKE_FD: AtomicI32 = AtomicI32::new(-1);
static PENDING: AtomicU64 = AtomicU64::new(0);

extern "C" fn on_signal(sig: libc::c_int) {
    // Async-signal-safe only: atomics, errno save/restore and write(2).
    let saved = errno();
    PENDING.fetch_or(1u64 << (sig as u32 & 63), Ordering::SeqCst);
    let fd = WAKE_FD.load(Ordering::SeqCst);
    if fd >= 0 {
        let b = sig as u8;
        // SAFETY: write of one byte from a live stack slot to our pipe.
        unsafe { libc::write(fd, (&b as *const u8).cast(), 1) };
    }
    set_errno(saved);
}

#[cfg(target_os = "macos")]
fn errno_ptr() -> *mut libc::c_int {
    // SAFETY: returns the calling thread's errno location.
    unsafe { libc::__error() }
}

#[cfg(target_os = "linux")]
fn errno_ptr() -> *mut libc::c_int {
    // SAFETY: returns the calling thread's errno location.
    unsafe { libc::__errno_location() }
}

fn errno() -> libc::c_int {
    // SAFETY: errno_ptr is the live thread-local errno.
    unsafe { *errno_ptr() }
}

fn set_errno(v: libc::c_int) {
    // SAFETY: errno_ptr is the live thread-local errno.
    unsafe { *errno_ptr() = v }
}

fn take_pending() -> u64 {
    PENDING.swap(0, Ordering::SeqCst)
}

const TERMINATING: [libc::c_int; 3] = [libc::SIGTERM, libc::SIGINT, libc::SIGHUP];

struct Signals {
    read_fd: libc::c_int,
}

impl Signals {
    fn install() -> std::io::Result<Self> {
        let mut fds = [0; 2];
        // SAFETY: fds is a writable two-int array.
        if unsafe { libc::pipe(fds.as_mut_ptr()) } != 0 {
            return Err(std::io::Error::last_os_error());
        }
        for fd in fds {
            // SAFETY: fcntl on descriptors we just created.
            unsafe {
                libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC);
                let flags = libc::fcntl(fd, libc::F_GETFL);
                libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK);
            }
        }
        WAKE_FD.store(fds[1], Ordering::SeqCst);
        // Background tty access must not stop the guardian once the agent
        // owns the foreground; the agent resets these before exec.
        // SAFETY: setting dispositions to SIG_IGN has no handler code.
        unsafe {
            libc::signal(libc::SIGTTOU, libc::SIG_IGN);
            libc::signal(libc::SIGTTIN, libc::SIG_IGN);
        }
        let mut signals = TERMINATING.to_vec();
        signals.push(libc::SIGCHLD);
        #[cfg(target_os = "linux")]
        signals.push(platform::PDEATH_SIGNAL);
        for sig in signals {
            // SAFETY: sigaction is plain old data; the handler is async-signal-safe.
            unsafe {
                let mut action: libc::sigaction = std::mem::zeroed();
                action.sa_sigaction = on_signal as *const () as libc::sighandler_t;
                action.sa_flags = libc::SA_RESTART | libc::SA_NOCLDSTOP;
                libc::sigemptyset(&mut action.sa_mask);
                if libc::sigaction(sig, &action, std::ptr::null_mut()) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
            }
        }
        // The daemon spawns the guardian with termination signals blocked so a
        // stop during startup cannot kill it unhandled; deliver them now.
        // SAFETY: sigset operations on a local, then unblock for this thread.
        unsafe {
            let mut set: libc::sigset_t = std::mem::zeroed();
            libc::sigemptyset(&mut set);
            for sig in TERMINATING {
                libc::sigaddset(&mut set, sig);
            }
            libc::sigprocmask(libc::SIG_UNBLOCK, &set, std::ptr::null_mut());
        }
        Ok(Self { read_fd: fds[0] })
    }

    /// Block until a signal, the extra fd, or the timeout.
    fn wait(&self, extra: Option<libc::c_int>, timeout: Duration) {
        let mut fds = vec![libc::pollfd {
            fd: self.read_fd,
            events: libc::POLLIN,
            revents: 0,
        }];
        if let Some(fd) = extra {
            fds.push(libc::pollfd {
                fd,
                events: libc::POLLIN,
                revents: 0,
            });
        }
        // SAFETY: fds is a live array of fds.len() pollfd entries.
        unsafe {
            libc::poll(
                fds.as_mut_ptr(),
                fds.len() as libc::nfds_t,
                timeout.as_millis() as libc::c_int,
            )
        };
        let mut sink = [0u8; 64];
        // SAFETY: draining our own non-blocking pipe into a stack buffer.
        while unsafe { libc::read(self.read_fd, sink.as_mut_ptr().cast(), sink.len()) } > 0 {}
    }
}

// ---- tracking -------------------------------------------------------------

pub(crate) struct Tracker {
    self_pid: i32,
    /// Run members are created after the guardian; older processes are
    /// never inspected for the token or lease.
    self_start: u64,
    lease: (u64, u64),
    parent: i32,
    uid: u32,
    token_entry: Vec<u8>,
    tracked: HashSet<(i32, u64)>,
    signalled: HashMap<(i32, u64), libc::c_int>,
    buf: Vec<u8>,
}

impl Tracker {
    pub(crate) fn new(parent: i32, token: &str, lease: (u64, u64)) -> Self {
        let self_pid = std::process::id() as i32;
        Self {
            self_pid,
            self_start: platform::process(self_pid).map_or(0, |p| p.start),
            lease,
            parent,
            // SAFETY: geteuid has no preconditions.
            uid: unsafe { libc::geteuid() },
            token_entry: format!("{RUN_TOKEN_ENV}={token}").into_bytes(),
            tracked: HashSet::new(),
            signalled: HashMap::new(),
            buf: Vec::new(),
        }
    }

    /// Record every same-uid process that descends from the guardian, or
    /// started after it and carries the exact token entry in its environment
    /// or holds the inherited lease descriptor.
    pub(crate) fn scan(&mut self) {
        let procs: Vec<ProcEntry> = platform::list_processes()
            .into_iter()
            .filter(|p| p.uid == self.uid && !p.zombie)
            .collect();
        let parents: HashMap<i32, i32> = procs.iter().map(|p| (p.pid, p.ppid)).collect();
        for p in &procs {
            if p.pid == self.self_pid
                || p.pid == self.parent
                || self.tracked.contains(&(p.pid, p.start))
            {
                continue;
            }
            let marked = chain_reaches(&parents, p.pid, self.self_pid)
                || (p.start >= self.self_start
                    && (platform::environment(p.pid, &mut self.buf)
                        .is_some_and(|env| env_block_has_entry(&env, &self.token_entry))
                        || platform::holds_vnode(p.pid, self.lease)));
            if marked {
                self.tracked.insert((p.pid, p.start));
            }
        }
    }

    /// A tracked process that still exists with the same start time is alive,
    /// whatever its uid now reads: an unverifiable survivor must never count as
    /// proof of termination. Signals still only succeed for same-uid processes.
    fn is_alive(&self, (pid, start): (i32, u64)) -> bool {
        platform::process(pid).is_some_and(|p| p.start == start && !p.zombie)
    }

    pub(crate) fn alive(&self) -> Vec<(i32, u64)> {
        let mut alive: Vec<_> = self
            .tracked
            .iter()
            .copied()
            .filter(|&t| self.is_alive(t))
            .collect();
        alive.sort_unstable();
        alive
    }

    /// Signal each live tracked process once per signal, re-verifying its
    /// (pid, start) immediately before kill(2) so a recycled pid is skipped.
    fn signal_alive(&mut self, sig: libc::c_int) {
        for t in self.alive() {
            if self.signalled.get(&t) == Some(&sig) || t.0 == self.self_pid || t.0 == self.parent {
                continue;
            }
            if self.is_alive(t) {
                // SAFETY: kill(2) on a verified live same-uid run process.
                unsafe { libc::kill(t.0, sig) };
                self.signalled.insert(t, sig);
            }
        }
    }
}

// ---- agent ----------------------------------------------------------------

struct Agent {
    pid: i32,
    status: Option<i32>,
}

impl Agent {
    /// Reap every exited child (the agent, and on Linux adopted orphans).
    fn reap(&mut self) {
        loop {
            let mut status = 0;
            // SAFETY: non-blocking wait on our own children.
            let pid = unsafe { libc::waitpid(-1, &mut status, libc::WNOHANG) };
            if pid <= 0 {
                return;
            }
            if pid == self.pid {
                self.status = Some(decode_status(status));
            }
        }
    }

    /// The agent's pgid is safe to target only while it is unreaped.
    fn signal_group(&self, sig: libc::c_int) {
        if self.status.is_none() {
            // SAFETY: the unreaped agent pins its pid/pgid against reuse.
            unsafe { libc::killpg(self.pid, sig) };
        }
    }
}

fn decode_status(status: libc::c_int) -> i32 {
    if libc::WIFEXITED(status) {
        libc::WEXITSTATUS(status)
    } else if libc::WIFSIGNALED(status) {
        128 + libc::WTERMSIG(status)
    } else {
        1
    }
}

fn parent_alive(parent: i32) -> bool {
    // SAFETY: getppid has no preconditions.
    unsafe { libc::getppid() == parent }
}

// ---- run ------------------------------------------------------------------

pub(crate) struct RunResult {
    pub trigger: Trigger,
    pub signal: Option<i32>,
    pub agent_exit: Option<i32>,
    pub survivors: Vec<u32>,
}

/// Errors before the agent is forked are returned; afterwards the run always
/// completes with a result.
pub(crate) fn supervise(
    args: &GuardianArgs,
    token: &str,
    lease: (u64, u64),
) -> Result<RunResult, String> {
    let parent = args.parent as i32;
    let signals = Signals::install().map_err(|e| format!("signal setup failed: {e}"))?;
    platform::become_subreaper();
    let mut watch =
        platform::ParentWatch::arm(parent).map_err(|e| format!("parent watch failed: {e}"))?;
    // Re-check after registration closes the spawn/registration race.
    if watch.parent_exited() || !parent_alive(parent) {
        return Ok(RunResult {
            trigger: Trigger::DaemonExit,
            signal: None,
            agent_exit: None,
            survivors: Vec::new(),
        });
    }

    let mut tracker = Tracker::new(parent, token, lease);
    let mut agent = match spawn_agent(args, token) {
        Ok(pid) => Agent { pid, status: None },
        Err(e) => {
            eprintln!("pu-engine guardian: failed to start agent: {e}");
            return Ok(RunResult {
                trigger: Trigger::AgentExit,
                signal: None,
                agent_exit: Some(127),
                survivors: Vec::new(),
            });
        }
    };

    let (trigger, signal) = loop {
        tracker.scan();
        signals.wait(watch.poll_fd(), SCAN_INTERVAL);
        agent.reap();
        let pending = take_pending();
        if watch.parent_exited() || !parent_alive(parent) {
            break (Trigger::DaemonExit, None);
        }
        if let Some(sig) = TERMINATING
            .iter()
            .copied()
            .find(|&s| pending & (1u64 << s) != 0)
        {
            if sig == libc::SIGHUP && parent_dies_within(&mut watch, parent, HANGUP_SETTLE) {
                break (Trigger::DaemonExit, None);
            }
            break (Trigger::Signal, Some(sig));
        }
        if agent.status.is_some() {
            break (Trigger::AgentExit, None);
        }
    };

    let survivors = terminate(&mut tracker, &mut agent, args.grace);
    Ok(RunResult {
        trigger,
        signal,
        agent_exit: agent.status,
        survivors,
    })
}

fn parent_dies_within(watch: &mut platform::ParentWatch, parent: i32, window: Duration) -> bool {
    let deadline = Instant::now() + window;
    while Instant::now() < deadline {
        if watch.parent_exited() || !parent_alive(parent) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    false
}

/// SIGTERM everything tracked, wait out the grace period (re-scanning and
/// terminating late arrivals), SIGKILL survivors, then re-scan until nothing
/// tracked or token-marked is alive or the deadline passes.
fn terminate(tracker: &mut Tracker, agent: &mut Agent, grace: Duration) -> Vec<u32> {
    let started = Instant::now();
    tracker.scan();
    agent.signal_group(libc::SIGTERM);
    tracker.signal_alive(libc::SIGTERM);
    while started.elapsed() < grace {
        agent.reap();
        tracker.scan();
        tracker.signal_alive(libc::SIGTERM);
        if tracker.alive().is_empty() {
            break;
        }
        std::thread::sleep(TERMINATION_TICK);
    }
    let deadline = Instant::now() + KILL_DEADLINE;
    loop {
        agent.reap();
        agent.signal_group(libc::SIGKILL);
        tracker.scan();
        tracker.signal_alive(libc::SIGKILL);
        agent.reap();
        let alive = tracker.alive();
        if alive.is_empty() || Instant::now() >= deadline {
            return alive.into_iter().map(|(pid, _)| pid as u32).collect();
        }
        std::thread::sleep(TERMINATION_TICK);
    }
}

fn spawn_agent(args: &GuardianArgs, token: &str) -> std::io::Result<i32> {
    // Take the terminal only when the guardian currently owns the foreground.
    // SAFETY: isatty/tcgetpgrp/getpgrp only inspect our own state.
    let take_tty = unsafe { libc::isatty(0) == 1 && libc::tcgetpgrp(0) == libc::getpgrp() };
    let mut cmd = Command::new(&args.command);
    cmd.args(&args.args)
        .env_remove(&args.token_env)
        .env(RUN_TOKEN_ENV, token);
    // SAFETY: the closure only calls async-signal-safe libc functions.
    unsafe {
        cmd.pre_exec(move || {
            libc::setpgid(0, 0);
            if take_tty {
                libc::tcsetpgrp(0, libc::getpid());
            }
            // SIG_IGN survives exec; restore the agent's defaults.
            libc::signal(libc::SIGTTOU, libc::SIG_DFL);
            libc::signal(libc::SIGTTIN, libc::SIG_DFL);
            Ok(())
        });
    }
    let child = cmd.spawn()?;
    let pid = child.id() as i32;
    // Mirror setpgid/tcsetpgrp in the parent to close the exec race.
    // SAFETY: both calls target the child we just forked.
    unsafe {
        libc::setpgid(pid, pid);
        if take_tty {
            libc::tcsetpgrp(0, pid);
        }
    }
    // Reaping is done with waitpid(-1); the std handle is not used again.
    drop(child);
    Ok(pid)
}
