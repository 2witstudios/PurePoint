# Mesh run guardian

Worker-side process-lifetime supervisor for remotely launched agent runs on a
**trusted personal fleet**. Agents are trusted runtimes, not adversaries: the
guardian contains accidental escapes (daemonized dev servers, SIGHUP-ignoring
children, double-forked `setsid` descendants). It does not contain a process
that is deliberately trying to escape.

## Why

The daemon spawns agents on a PTY (`NativePtyHost::spawn`: fork, `setsid()`,
login tty, close fds > 2, exec). If the daemon is SIGKILLed on macOS,
descendants keep running: a SIGHUP-ignoring session leader and double-forked
`setsid` descendants (PPID 1) survive. macOS has no unprivileged "kill the
whole tree" primitive (launchd is PGID-scoped, kqueue `NOTE_TRACK` is
`ENOTSUP`, coalitions need privilege), so every run executes under a guardian.

## Contract

```
pu-engine --mesh-guardian --run <run_id> --token-env <VAR> --parent <pid> \
          --outcome-dir <dir> [--grace-ms <ms>] -- <command> [args...]
```

Dispatched at the very top of `main.rs`, before any daemon setup (no tokio
runtime, tracing, state dir or daemon lock). Synchronous and single-threaded.

1. **Validation before any fork.** `run_id` is 1..=64 of `[A-Za-z0-9_-]`; the
   token (read from the env var named by `--token-env`, never argv) is
   32..=1024 hex chars; the guardian refuses if the token appears anywhere on
   its argv. `--outcome-dir` must be an existing directory owned by the current
   uid, mode `0700`, no symlink leaf, no ACL grants, safe ancestors
   (`pu_core::private_fs::PrivateDir`). An existing outcome for the run id is
   refused. Refusals exit `2` (usage) or `3` (outcome dir) without forking.
2. **Token handling.** The guardian removes `--token-env` from its own
   environment and starts the agent with `PU_MESH_RUN_TOKEN=<token>` added and
   the guardian's token variable removed. Descendants inherit the token.
3. **Agent launch.** The guardian is the PTY session leader (what the daemon
   execs). The agent is forked into the same session, in its own process
   group; when the guardian owns the terminal foreground the agent's group is
   made the foreground group (so it reads the terminal; verified through the
   real `NativePtyHost` path). SIGWINCH needs nothing.
4. **Membership channels** (all scoped to same-uid processes):
   - *Ancestry*: PPID chain reaches the guardian.
   - *Token*: the environment contains exactly `PU_MESH_RUN_TOKEN=<token>`.
   - *Lease*: the process holds an inherited descriptor on the guardian's
     lease, an unlinked `0600` file created in the outcome dir and identified
     by `(dev, ino)`. Added because macOS 26 no longer exposes the
     environment of Apple platform binaries (`/bin/sh`, `/bin/sleep`,
     `/usr/bin/perl`, ...) via `KERN_PROCARGS2`, even to the same uid.
   Token and lease are only checked for processes that started after the
   guardian. Every match is recorded as `(pid, start_time)`; scans run every
   250 ms while the run is alive and continuously during termination.
5. **Triggers.** (a) daemon death, (b) agent exit, (c) SIGTERM/SIGINT/SIGHUP.
   - macOS: kqueue `EVFILT_PROC`/`NOTE_EXIT` on `--parent`, then `getppid()`
     re-checked after registration to close the race.
   - Linux: `PR_SET_PDEATHSIG` + `getppid()` re-check, and
     `PR_SET_CHILD_SUBREAPER` so orphans reparent to the guardian.
   - All platforms: `getppid()` is also polled on every wake (≤ 250 ms).
   - A SIGHUP is classified as `daemon_exit` if the parent is seen to die
     within 150 ms (PTY hangup caused by the daemon dying).
6. **Termination.** `killpg(agent, SIGTERM)` (forwarding), SIGTERM to every
   live tracked process, grace period (default 2000 ms, `--grace-ms`), SIGKILL
   to the agent group and every survivor, then re-scan until nothing tracked
   or marked is alive, bounded by a 5 s deadline. Before every signal the
   `(pid, start_time)` is re-verified, so a recycled pid is never signalled.
   The guardian never signals itself or `--parent`.
7. **Outcome.** `<outcome-dir>/<run_id>.outcome.json`:
   `{run_id, token_digest (sha256 hex), verified, agent_exit, trigger
   ("daemon_exit"|"agent_exit"|"signal"), survivors, finished_at}` written to
   an exclusive `0600` no-follow temp file, fsynced, renamed into place only
   if no outcome exists. `verified` means no tracked/marked process survived.
   The raw token is never written.
8. **Exit status.** The agent's exit code, or `128+signal` if it was killed
   by a signal. If the agent was never reaped: `128+signal` for a signal
   trigger, else `1`.

## Library API

```rust
pub const RUN_TOKEN_ENV: &str;       // "PU_MESH_RUN_TOKEN"
pub const GUARDIAN_TOKEN_ENV: &str;  // "PU_MESH_GUARDIAN_TOKEN"
pub struct GuardianLaunch { pub run_id: String, pub token: String, pub outcome_dir: PathBuf }
pub fn wrap_spawn(launch: &GuardianLaunch, exe: &Path, parent: u32, command: String, args: Vec<String>)
    -> (String, Vec<String>, Vec<(String, String)>);
pub fn new_token() -> String;                                 // 256-bit hex (getentropy)
pub fn supervision_identity(token: &str) -> (String, String); // ("guardian-v1-macos"|"guardian-v1-linux", sha256 hex)
pub fn read_outcome(outcome_dir: &Path, run_id: &str, token: &str) -> io::Result<Option<Outcome>>;
pub fn run_guardian(args: &[String]) -> i32;
```

`read_outcome` returns `Ok(None)` until the outcome exists, and an error if
the directory or file is unsafe, the JSON is malformed or over 64 KiB, the run
id differs, or the token digest does not match (constant-time compare).

Integration note for the engine: `wrap_spawn` output drops into a
`SpawnConfig` (`command`, `args`, `env`). `NativePtyHost::kill` sends
SIGTERM to the guardian's process group (the guardian alone, since the agent
has its own group); its grace must exceed the guardian's grace plus the scan
overhead, or the daemon's SIGKILL will interrupt termination.

## Residual limits (honest)

- **Deliberate escape.** A process that scrubs its environment (`env -i`),
  closes inherited descriptors, and leaves the guardian's ancestry *before the
  guardian observes it* is not linked to the run. If it was observed while any
  channel held (e.g. still a descendant), its `(pid, start)` is remembered and
  it is still killed (tested). The window is the scan interval (250 ms).
- **macOS platform binaries.** Their environment is invisible on macOS 26, so
  for them the token channel is inert; the lease and ancestry channels carry
  the load. A platform binary that closes all inherited fds and double-forks
  within one scan interval escapes.
- **Guardian SIGKILL.** If the guardian itself is SIGKILLed, nothing cleans up
  and no outcome is written; consumers must treat a missing outcome as
  unverified.
- **Same-uid only.** Processes that change uid (setuid helpers) are invisible.
- **PID-check race.** Between verifying `(pid, start)` and `kill(2)` there is
  a microsecond window in which a pid could be recycled; mitigated by the
  start-time check and the rarity of pid wrap on a live run.
- **Token on the guardian's original env block.** `remove_var` does not erase
  the exec-time env strings, so the guardian's token variable remains visible
  to same-uid readers of its procargs/`/proc/<pid>/environ` (never to other
  uids, and never on argv).
- **Linux.** The Linux backend (`/proc`, PDEATHSIG, subreaper) compiles only
  under `cfg(target_os = "linux")` and was **not built or run** in this
  branch's evidence. PDEATHSIG uses `SIGUSR2` rather than `SIGTERM` because it
  fires on death of the *forking thread*, which in the multi-threaded daemon
  is not daemon death; the guardian treats it only as a wake-up and trusts
  `getppid()`.

## Evidence (macOS 26.6.2, arm64)

`crates/pu-engine/tests/mesh_guardian.rs` runs the compiled binary against
real processes, each carrying a unique argv marker that a drop guard SIGKILLs
on failure. Covered: daemon SIGKILL with SIGTERM/SIGHUP-ignoring child and
double-forked `setsid` descendant; agent exit with detached descendant (exit
code propagated); SIGTERM forwarding; scrubbed + `setsid` + fd-closed +
reparented descendant observed beforehand; a detached, fd-closed, non-platform
descendant found by token env alone; foreign same-uid processes with no token,
a different token, or a token-prefixed value are never signalled; unsafe
outcome dirs (symlink, `0777`, not owned) refused before forking and rejected
by the reader; tampered digest / wrong token rejected; token absent from
guardian and agent argv; real `NativePtyHost` path (terminal input reaches
the agent, daemon `kill` contains the tree). Mutation checks: disabling the
lease, token or tracked-`(pid,start)` channel, or relaxing exact token match
to a prefix match, each fails the corresponding test.

Measured: SIGKILL of the fake daemon to outcome on disk, with a 500 ms grace
and a SIGTERM-ignoring child, 519–646 ms over 7 runs (detection + scan overhead under
~150 ms).
