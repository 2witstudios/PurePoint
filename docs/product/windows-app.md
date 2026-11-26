# Windows App

**Maturity: SPECIFIED** | ID Prefix: WINAPP | Dependencies: `product/desktop-app.md`, `product/machines.md`, `product/ipad-app.md`, `architecture/daemon-engine.md`, `architecture/ipc-api.md`, `architecture/distribution.md`, `architecture/desktop-app-integration.md`, `research/windows-conpty.md`, `research/windows-ipc.md`, `reference/windows-parity-matrix.md`

## Purpose

The Windows app is the macOS app on Windows. It has the Mac app's full feature set on Windows 11 (22H2 or later, x64 and arm64), and it runs against a native local daemon: `pu-engine.exe` and `pu.exe` built for Windows, with no WSL. Parity covers:

- sidebar outline, pane grid and tabs;
- interactive terminals, editor, file tree, diffs and review;
- command palette, hotkeys and menus;
- Schedule, Agents hub, Triggers, Channels and Settings;
- remote machines over SSH;
- Point Guard, notifications, CLI install and in-place updates.

It also covers everything the app depends on in the engine and CLI: ConPTY agent hosting, a private named-pipe daemon endpoint, Windows shells and environments, owner-only files, autostart, the install layout, Windows path rules and worktree removal while files are open.

The Windows app is a separate app (`apps/purepoint-windows`, Tauri 2 + React + TypeScript), not a target of the Xcode project. Behaviour parity is tracked file by file in `docs/reference/windows-parity-matrix.md`. The daemon protocol is the same on every platform, so a Windows app, a Mac app, an iPad and the CLI are interchangeable clients of any daemon.

## Conceptual Model

```
PurePoint for Windows (apps/purepoint-windows)
  Tauri 2 shell          one native window per workspace window; WebView2
  ui/  (React + TS)      views, Zustand stores mirroring the Swift state classes,
                         xterm.js terminals, generated protocol types (ts-rs)
  src-tauri/ (Rust)      daemon client, attach and subscription streams, watchers,
                         git, conversation indexes, secrets, CLI installer
    └─ pu-transport ──→ \\.\pipe\purepoint-<hash16>-daemon ──→ pu-engine.exe (local)
    └─ ssh.exe <host> pu daemon proxy  (one process per daemon connection) ──→ remote daemon

pu-engine.exe (native Windows daemon, %USERPROFILE%\.pu\bin)
  IPC server             private named pipe, same NDJSON protocol as the Unix socket
  PTY host               ConPTY + suspended spawn + Job Object per agent
  Files                  private_fs: protected owner-only DACL
  Lifecycle              daemon.sock.lock (fs4), daemon.sock.pid, HKCU Run autostart
```

- **Socket path stays the stem.** `%USERPROFILE%\.pu\daemon.sock` is never created as a socket on Windows, but it still names the daemon. The pipe name is derived from it (WINAPP-003). The `daemon.sock.pid`, `daemon.sock.lock` and `daemon.sock.projects.json` files sit beside it exactly as on Unix.
- **Agent = ConPTY + Job Object.** Each agent is a pseudoconsole, a root process created suspended, and a kill-on-close Job Object that holds the root's whole tree. The Job Object replaces the Unix session and process group. Closing it kills the tree, even when the daemon crashes.
- **Seam, not fork.** The engine keeps one implementation. Windows bodies sit behind `#[cfg(windows)]`, behind the `PtyHandle` trait (`write`, `resize`, `size`, `reader`, `pid`), behind `pu-transport` and behind `pu-core::private_fs`. Unix behaviour and the wire format do not change.
- **Port, not re-design.** Pure Swift logic (models, reconcilers, state classes, view models) is ported to TypeScript one file to one file. Anything that touches the OS, git, history files or the daemon goes into the Rust backend.

## Research Notes

**Researched: 2026-11-25** on Windows 11 build 26300, unelevated. The full spikes are in `docs/research/windows-conpty.md` (WIN-1.1, `spikes/conpty`) and `docs/research/windows-ipc.md` (WIN-1.2, `spikes/pipes`). The transport and private-file rules are already implemented in `crates/pu-transport` and `crates/pu-core/src/private_fs.rs`.

ConPTY (WIN-1.1):

- The spawn sequence works: pipes, `CreatePseudoConsole`, attribute list, `CREATE_SUSPENDED`, Job Object, `ResumeThread`. Claude, Codex, pwsh and cmd render correctly through the vendored `vt100`, and the delivery heuristics (`input_phase`, `find_input_box`, `squash`) work unchanged on the parsed screens.
- The MSIX (Store) build of pwsh creates its children outside the job unless `PROC_THREAD_ATTRIBUTE_DESKTOP_APP_POLICY = PROCESS_CREATION_DESKTOP_APP_BREAKAWAY_DISABLE_PROCESS_TREE` is set. With the attribute set, `TerminateJobObject` left 0 survivors. Without it, all 3 grandchildren survived.
- Graceful stop: Claude exits 0 on `\x03`, 300 ms, `\x03`. Codex exits on a single `\x03`. A second `\x03` while a `.cmd` wrapper is in the tree leaves cmd alive at a prompt, and a console Ctrl-C event shows `Terminate batch job (Y/N)?`. `ClosePseudoConsole` stopped every case within 2 s.
- `.cmd`/`.bat` shims need `cmd.exe /d /s /c "<quoted line>"` with the quoting rules in the spike. CRT-style `\"` escaping is injectable. A simple forwarder shim (`@"<exe>" %*`) can be resolved to its target exe, which keeps cmd out of the tree.
- ConPTY has no `TIOCGWINSZ`. The host must remember the size it set and resize its vt100 parser at the moment of the resize.
- `NO_COLOR=1` in the daemon's environment strips Claude's dim placeholder, so `input_phase` misreads an empty box as a draft. The daemon must remove it from agent environments.
- Claude's session registry on Windows uses `pidDomain = "win32:" + lowercase(COMPUTERNAME)`. Claude hooks in exec form (`command` + `args`) run with no shell, while shell-form hooks run under Git Bash or pwsh depending on the machine.
- Passthrough mode (`0x8`) brings no benefit on 26300. Nothing may depend on it.

Named pipes (WIN-1.2):

- A protected DACL (`O:<me>D:P(A;;GA;;;<me>)(A;;GA;;;SY)`), `FILE_FLAG_FIRST_PIPE_INSTANCE` and `PIPE_REJECT_REMOTE_CLIENTS` make the pipe private. A second first-instance create fails with error 5.
- Pipe names are global, so clients must verify the server. The SID check stops other users. A same-user impostor passes the SID check, and the PID in `daemon.sock.pid` catches it.
- The daemon's NDJSON limits port unchanged: 1100 clients gave 1024 accepted and 76 `BUSY`, once the `BUSY` line was written with an awaited `write_all`. `PeekNamedPipe` sees a disconnect 31 ms after it happens. Node's `net.createConnection('\\\\.\\pipe\\…')` works.
- Autostart: `schtasks /SC ONLOGON` is denied unelevated. A COM per-user logon task works, and the HKCU `Run` value works.
- Normalizing the path, then hashing it, gives one name for every spelling of a home (case, `/`, `\\?\`, `..`, junction, 8.3) and different names for different homes.

Alternatives considered:

- **WSL-hosted daemon.** Rejected by the owner (2026-11-25). Agents must run in the user's Windows session, with Windows `claude.exe` and `codex.exe`, and WSL is an install dependency.
- **`portable-pty`.** Rejected (plan DEC-1). It cannot spawn suspended, so children spawned before the job assignment escape `pu kill`.
- **A loopback TCP listener.** Rejected. It needs a credential in the protocol, while a named pipe gives an OS-enforced ACL and a peer identity for free.
- **GPUI, WinUI 3 or Electron for the UI.** Rejected by the owner in favour of Tauri 2. It is a smaller runtime than Electron (it uses the system WebView2), it uses web skills the team already has, and its backend is Rust, so it can share `pu-core` and `pu-transport`.
- **A Windows service for the daemon.** Rejected. Services run in session 0, outside the user's interactive session and environment, where agent CLIs and their credentials are not available.

Not proven, and owned by later leaves: Codex hooks on Windows (source only, WIN-3.9), graceful stop mid-turn (WIN-3.3), whether autostart fires at a real logon (WIN-4.3), and other-user or elevated clients (WIN-3.4). Each is covered by a requirement below whose test settles it.

## Decisions

! [WINAPP-001] Scope is the macOS app's full feature set on Windows 11 22H2+ (x64 and arm64) with a native local daemon — the owner chose a native port with full parity and no WSL (2026-11-25). Windows 10 is not supported, which keeps ConPTY behaviour predictable (plan DEC-12).

! [WINAPP-002] Host agents in ConPTY directly on `windows-sys`, with a suspended spawn into a kill-on-close Job Object — `portable-pty` cannot spawn suspended, and a child that runs before the job assignment can escape it (plan DEC-1, WIN-1.1 check 1).

! [WINAPP-003] The daemon endpoint is `\\.\pipe\purepoint-<hash16>-daemon`, where `<hash16>` is the first 8 bytes of SHA-256 over the normalized socket path, in lowercase hex. The path is not lowercased — hashing the socket path, not just `~/.pu`, gives every home, test home and custom `--socket` its own pipe with no `PU_HOME` variable, and canonical casing is already stable, while lowercasing would merge distinct folders in case-sensitive directories (plan DEC-2 as amended, `pu-transport/src/naming.rs`).

! [WINAPP-004] Privacy is enforced at both ends of the pipe. The pipe has an owner-only protected DACL, a first-instance create and remote clients rejected. The server checks each client's user SID. The client checks that the server's user SID equals its own and that the server's PID equals a live PID in `<socket>.pid` — pipe names are global, so this replaces the 0700 directory that protects a Unix socket. The PID check stops a same-user impostor that the SID check cannot (WIN-1.2 criterion 7).

! [WINAPP-005] Stop agents gracefully and per agent, then fall back to the pseudoconsole, then to the job. Claude gets `\x03`, 300 ms, `\x03`. Codex gets one `\x03`. Every agent then gets `ClosePseudoConsole`, and finally `TerminateJobObject` — these are the only methods WIN-1.1 measured to exit cleanly. A second `\x03` or a console control event is unsafe while a `.cmd` wrapper is in the tree. `ClosePseudoConsole` stopped every case in under 2 s.

! [WINAPP-006] The default shell is `pwsh.exe`, then `powershell.exe`, then `%COMSPEC%`. A `.cmd`/`.bat` agent shim whose body is a single forwarder line is launched as its target exe. Any other shim goes through `cmd.exe /d /s /c` with the WIN-1.1 quoting — pwsh is the modern default, and keeping cmd out of the tree keeps Ctrl-C stops reliable (WIN-1.1 section 6). The quoting rules are the only ones the spike proved injection-safe (plan DEC-4).

! [WINAPP-007] Agent environments start from the daemon's environment. The current `HKCU\Environment` and system `PATH` are merged in, and the Windows tool directories are added. `NO_COLOR`, `CLAUDECODE`, `CLAUDE_CODE_*` and `WT_*` are removed, and `TERM=xterm-256color` and `COLORTERM=truecolor` are set — Windows has no login shell to replay, so PATH edits made since logon must come from the registry, and `NO_COLOR` breaks the screen heuristics (WIN-1.1 section 1).

! [WINAPP-008] Private files and directories get a protected DACL with no inherited entries. It grants full control to the current user and to SYSTEM, and read to Administrators. `check_private` rejects any other allow entry and any owner other than the user, SYSTEM or Administrators, and it refuses reparse points where a private file is expected — this is the Windows meaning of 0600/0700 plus `O_NOFOLLOW`. SYSTEM and Administrators already control the machine (`pu-core/src/private_fs.rs`, `win_security.rs`).

! [WINAPP-009] Single instance comes from an exclusive `fs4` lock on `daemon.sock.lock`, and the PID file is `daemon.sock.pid` on every platform. The CLI spawns the daemon detached (`DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW`, breaking away from an inherited job when the job allows it). `--managed --parent-pid <pid>` makes the daemon exit when its parent's process handle signals — `flock`, `setsid` and the `parent_id()` poll have no Windows meaning. These are their direct equivalents.

! [WINAPP-010] `pu-engine.exe` is built for the Windows GUI subsystem. It has no console, and its stderr goes to `~/.pu/daemon.log` through handles its launcher passes — a console-subsystem daemon started from a `Run` value or by the app would open a console window. The daemon reads no input from a console and needs none.

! [WINAPP-011] Autostart is an `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` value. A per-user COM logon task is used only if restart-on-failure or a start delay is needed. There is no Windows service — `schtasks /SC ONLOGON` is denied unelevated, the Run value needs no COM and shows up in Settings > Startup apps, and a service would run outside the user's session (plan DEC-6 as amended, WIN-1.2 Autostart).

! [WINAPP-012] `~/.pu` is `%USERPROFILE%\.pu`, and `pu.exe` and `pu-engine.exe` install to `%USERPROFILE%\.pu\bin`. App-private state (windows, selection, preferences) lives in the Tauri app data directory — this matches the README and the Unix layout, so docs, scripts and the remote start command stay the same (plan DEC-3).

! [WINAPP-013] Remote machines are reached by system `ssh.exe` running `pu daemon proxy` on the machine, one `ssh.exe` process per daemon connection — Win32-OpenSSH has no `ControlMaster`, and the Unix-socket `ssh -L` forward has no Windows endpoint to bind. The exec relay is the transport the iPad already uses (plan DEC-5, IPAD-004).

! [WINAPP-014] Paths are stored without `\\?\` prefixes. They are canonicalized through `dunce`, with `\` separators, an uppercase drive letter and no trailing separator. Paths are compared case-insensitively — manifests, `.projects.json` and protocol payloads are shared with other clients and must not drift. NTFS directories are case-insensitive by default.

! [WINAPP-015] Worktree removal tolerates sharing locks. The app releases its watchers first. The daemon waits for the agent job to empty, then removes the worktree through a bounded retry, and names the locked path when it gives up — on Windows, an open handle (a watcher, an editor, Defender, an exiting agent) makes delete and rename fail with sharing violations that Unix never raises (WIN-3.13).

! [WINAPP-016] Agent signals use a per-launch named pipe in place of the `UnixDatagram`, under the same `PU_SIGNAL_SOCKET` and `PU_SIGNAL_TOKEN` variables. Claude hooks are written in exec form (`command` + `args`) — no single shell-form string works under both Git Bash and pwsh, and exec form passes arguments exactly (WIN-1.1 section 9, WIN-1.2 signal pipe).

! [WINAPP-017] The app is Tauri 2 with a Rust backend, React 19 + TypeScript (strict) in the front end, and xterm.js terminals. `src-tauri` is its own Cargo workspace, excluded from the root, and depends on `pu-core` and `pu-transport` by path — this was the owner's choice (2026-11-25). Linux and macOS `cargo` runs never build Tauri (plan DEC-7).

! [WINAPP-018] Protocol types are generated from `pu-core` with `ts-rs` behind an optional `ts` feature, and CI fails on drift — one source of truth for the wire format. With the feature off, pu-core is unchanged.

! [WINAPP-019] UI state lives in Zustand stores that mirror the Swift `@Observable` state classes one to one — a Swift change then maps to a single TypeScript file (plan DEC-14).

! [WINAPP-020] Hotkeys follow Windows Terminal conventions, and the DEC-8 default table (Interfaces) is copied exactly. Chords match on `KeyboardEvent.code`, no default uses Ctrl+Alt, and every binding is remappable — plain Ctrl chords must reach a focused terminal, and Ctrl+Alt is AltGr on many layouts (plan DEC-8).

! [WINAPP-021] Terminals and the editor use `Cascadia Mono` 13 px, and UI text uses Segoe UI Variable — Cascadia ships with Windows 11 and is the closest match to the Mac's Menlo 13 (plan DEC-9).

! [WINAPP-022] The window has a custom title bar: `decorations: false`, a drag region, and caption buttons drawn in the Windows 11 style, whose hit-testing returns `HTMAXBUTTON` so Snap Layouts work — the sidebar then reaches the top edge like the Mac's transparent title bar (plan DEC-10).

! [WINAPP-023] SF Symbols map to Lucide icons through one table, the SF Symbols table in the parity matrix, generated into `ui/src/theme/icons.ts`, and a test fails on any unmapped name — one auditable mapping, and no symbol silently renders blank (plan DEC-13).

! [WINAPP-024] Mac-only technology is replaced, not emulated: Sparkle by the Tauri updater, Keychain by Credential Manager, `NSOpenPanel`/`NSWorkspace` by the Tauri dialog and opener plugins, and Citadel SSH by `ssh.exe`. The legacy chat files and the iOS-only files are not ported (Exclusions) — each Mac API has a maintained Windows counterpart, and the excluded files are unused on the Mac or iPad-only (plan DEC-11).

! [WINAPP-025] Unix behaviour and the wire format are unchanged. Every Windows change is `cfg`-gated, and `PROTOCOL_VERSION` stays 14 — the port must not regress the Mac or Linux servers, and a Windows daemon must be a drop-in peer for every existing client.

## Requirements

### Scope

- REQ-WINAPP-001: Given Windows 11 22H2 or later on x64 or arm64, should install and run the app, `pu.exe` and `pu-engine.exe` with no WSL, Git Bash or admin rights. Given Windows 10, should refuse to install with a message naming the minimum version.
- REQ-WINAPP-002: Given the parity matrix, should give every non-excluded macOS source file a Windows counterpart, and the release gate should pass with no `todo` rows (`check-parity-matrix.mjs --no-todo`).
- REQ-WINAPP-003: Given the app, should present the Mac shell: sidebar outline, pane grid with tab stacks, workspace files sidebar, command palette, Schedule, Agents hub, Triggers, Channels, Settings and Point Guard, driven by stores that mirror the Mac state classes.

### IPC endpoint

- REQ-WINAPP-004: Given a socket path, should name the daemon pipe `\\.\pipe\purepoint-<hash16>-daemon` per WINAPP-003. Two homes give two names, and every spelling of one path gives one name (case, `/`, trailing `\`, `\\?\`, `..`, junction, 8.3).
- REQ-WINAPP-005: Given the daemon binding its pipe, should create the first instance with `FILE_FLAG_FIRST_PIPE_INSTANCE`, `PIPE_REJECT_REMOTE_CLIENTS` and the pipe SDDL (Interfaces). Given error 5 on that create, should report the endpoint as already owned and exit, and never serve on it.
- REQ-WINAPP-006: Given an accepted connection whose client token's user SID differs from the daemon's, should close it before it takes a connection permit.
- REQ-WINAPP-007: Given `connect`, should refuse a server whose user SID differs from the caller's, or whose PID differs from a live PID in `<socket>.pid`, before sending any byte. Given `ERROR_PIPE_BUSY`, should retry for up to 2 s.
- REQ-WINAPP-008: Given the 1 MB frame limit, the 30 s first-request timeout, the 1024-connection cap with one `BUSY` line, and mid-operation disconnect detection, should behave exactly as on Unix, with the same tests running on both.
- REQ-WINAPP-009: Given a listening daemon, should always keep one listening pipe instance, creating the next one before it hands off a connected one, so clients never see `ERROR_FILE_NOT_FOUND` while the daemon is up.

### PTY host

- REQ-WINAPP-010: Given a spawn, should follow the spawn sequence in Interfaces: suspended, `PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE` and `PROC_THREAD_ATTRIBUTE_DESKTOP_APP_POLICY` set, null std handles, no inherited handles, assigned to a kill-on-close job, then resumed. It should apply the working directory and environment.
- REQ-WINAPP-011: Given an agent whose shell is the Store pwsh and which starts a long-running grandchild, should leave zero processes from that tree after `pu kill` (process-snapshot test).
- REQ-WINAPP-012: Given a running agent, should drain the output pipe on a dedicated thread into `OutputBuffer::write`, keeping the Unix chunking, replay and `resync` behaviour.
- REQ-WINAPP-013: Given `resize`, should call `ResizePseudoConsole`, resize the agent's vt100 parser at that point in the byte stream, and report the new size from `PtyHandle::size`, which returns the size last set.
- REQ-WINAPP-014: Given child exit, should report the code from `GetExitCodeProcess`, reinterpreted as `i32`, through the existing exit channel. It should then close the pseudoconsole so the reader reaches EOF.
- REQ-WINAPP-015: Given `kill` with a grace period, should run the stop sequence in Interfaces: the agent's keystrokes, then `ClosePseudoConsole`, then `TerminateJobObject(137)`, then a sweep of the descendants snapshotted before the stop. It should return within the grace period plus 1 s.
- REQ-WINAPP-016: Given a daemon crash, should still end every agent tree, through `KILL_ON_JOB_CLOSE`.
- REQ-WINAPP-017: Given `write_chunked` and `write_chunked_submit`, should keep their Unix timing: 8-byte chunks 6 ms apart, and 180 ms before `\r`.
- REQ-WINAPP-018: Given the delivery heuristics, should pass against the ConPTY fixtures recorded in WIN-1.1, reading parsed screens only and never matching raw bytes.

### Shells, commands and environment

- REQ-WINAPP-019: Given the `shell` sentinel or a `terminal` agent, should launch the first of `pwsh.exe`, `powershell.exe` and `%COMSPEC%` found.
- REQ-WINAPP-020: Given an agent command, should resolve it with PATHEXT (the `which` crate), so `claude`, `codex` and `opencode` are found as `.exe` or `.cmd`.
- REQ-WINAPP-021: Given a resolved `.cmd`/`.bat` whose only command line is a forwarder (`@"<exe>" %*`, with `%~dp0` and `%LOCALAPPDATA%`-style variables expanded) to an existing exe, should launch that exe directly with the agent's arguments. Given any other shim, should launch `cmd.exe /d /s /c "<quoted line>"` with the WIN-1.1 quoting, and reject an argument containing a newline or NUL.
- REQ-WINAPP-022: Given an agent environment, should apply WINAPP-007: merge `HKCU\Environment` and the system `PATH`, prepend the Windows tool directories (Interfaces), remove `NO_COLOR`, `CLAUDECODE`, `CLAUDE_CODE_*` and `WT_*`, and set `TERM` and `COLORTERM`. It should never run a login shell.
- REQ-WINAPP-023: Given an empty `HKCU\Environment` PATH, a PATH entry with spaces, or a `.cmd`-only agent, should still resolve and launch (unit tests).
- REQ-WINAPP-024: Given helper processes the daemon or the app spawns (git, gh, ssh, gates), should create them with `CREATE_NO_WINDOW`, so no console window flashes.

### Private files

- REQ-WINAPP-025: Given `create_private_dir`, `create_new_private_file`, `open_private_file` or `write_private_file`, should apply the private SDDL (Interfaces) with inheritance blocked. Directories pass the ACEs to their children.
- REQ-WINAPP-026: Given `check_private` on a path with an allow entry for `Everyone`, `Users`, `Authenticated Users` or any other user, or an owner other than the user, SYSTEM or Administrators, should reject it.
- REQ-WINAPP-027: Given a symlink or junction where a private file or directory is expected, should refuse to follow it (`FILE_FLAG_OPEN_REPARSE_POINT` plus an attribute check).
- REQ-WINAPP-028: Given an atomic replace (`write_private_file`, `fs_ops` writes), should write a private temp file in the same directory, flush it, and rename it over the target through `fs_retry`.

### Daemon lifecycle and autostart

- REQ-WINAPP-029: Given a second `pu-engine` on the same socket path, should fail to take `daemon.sock.lock` and exit with the existing "already running" message, leaving the first daemon's pipe untouched.
- REQ-WINAPP-030: Given `pu` with no daemon answering, should spawn `pu-engine.exe` detached per WINAPP-009, with stdin and stdout null and stderr to `~/.pu/daemon.log`, and poll health with the Unix backoff. Closing the terminal that ran `pu` must not stop the daemon.
- REQ-WINAPP-031: Given the daemon binary search, should use the Unix rule. `PU_ENGINE_PATH` is the only candidate when set. Otherwise `pu-engine.exe` beside `pu.exe` is tried, then `%USERPROFILE%\.pu\bin`. `PATH` alone is never searched. A candidate whose DACL grants write to anyone other than the user, SYSTEM, Administrators or TrustedInstaller is rejected, and the error lists the reason.
- REQ-WINAPP-032: Given `pu-engine --managed --parent-pid <pid>`, should exit within 2 s of that process ending. Without `--parent-pid`, Unix keeps its `parent_id()` poll.
- REQ-WINAPP-033: Given `Shutdown`, or a console Ctrl-C, Ctrl-Break, close, logoff or shutdown event when one is delivered, should shut down gracefully as on SIGTERM: agents are stopped and left resumable, and the PID file is removed if it names this daemon.
- REQ-WINAPP-034: Given `pu daemon install [--now]` as a standard user, should write the Run value (Interfaces). `--now` also starts the daemon. `pu daemon uninstall` should delete the value, and `pu daemon status [--json]` should report it as the service state, the way the launchd and systemd managers do.
- REQ-WINAPP-035: Given a logon with the Run value present, should start a daemon with no console window. Given a daemon already running, the started one should exit through REQ-WINAPP-029.

### Install layout

- REQ-WINAPP-036: Given first launch of the app, or a newer bundled CLI, should copy `pu.exe` and `pu-engine.exe` into `%USERPROFILE%\.pu\bin`. It should add that directory to the user `PATH` in `HKCU\Environment` once, then broadcast `WM_SETTINGCHANGE`.
- REQ-WINAPP-037: Given a running `pu-engine.exe` (Windows locks running images), should rename it aside to `pu-engine.exe.old-<n>`, copy the new binary into place, ask the daemon to restart, and delete stale `.old-*` files on a later launch.
- REQ-WINAPP-038: Given `scripts/install.ps1`, should mirror `install.sh`: verify the minisign signature (or fail unless `PU_INSTALL_SKIP_VERIFY=1`), `Unblock-File` the binaries, install them into `%USERPROFILE%\.pu\bin` by rename, and print the PATH step.

### Paths

- REQ-WINAPP-039: Given any path the engine or CLI stores or sends (manifests, `.projects.json`, protocol payloads), should never contain `\\?\`. Canonicalization goes through one `dunce` helper.
- REQ-WINAPP-040: Given two spellings of one project or worktree path that differ only in case, separators or a trailing separator, should treat them as one identity, while storing the canonical on-disk casing.
- REQ-WINAPP-041: Given worktree paths over 260 characters, should create, use and remove them, with `core.longpaths=true` set on worktree git commands.
- REQ-WINAPP-042: Given a checkout with `core.autocrlf=true`, should keep tests green. `.gitattributes` pins `*.sh`, `.mjs` hooks and golden fixtures to `eol=lf`.

### Worktree deletion under file locks (WIN-3.13 contract)

- REQ-WINAPP-043: Given the app about to send `delete_worktree`, or a `kill` or `clean` that removes a worktree, should first drop every watcher whose root lies inside that worktree and wait for its handles to close. Given the request failing, should re-create those watchers.
- REQ-WINAPP-044: Given a worktree removal in the daemon, should stop its agents, then wait up to 5 s for each agent job's active process count to reach zero before running `git worktree remove --force`.
- REQ-WINAPP-045: Given `rename`, `remove_file` or `remove_dir_all` anywhere in pu-core, pu-engine or pu-cli, should go through `fs_retry`. On `ERROR_SHARING_VIOLATION`, `ERROR_ACCESS_DENIED` or `ERROR_LOCK_VIOLATION`, it retries at least 10 times over at least 2 s, clearing read-only attributes first. On Unix it is a passthrough.
- REQ-WINAPP-046: Given a file inside the worktree held open by another process past the retry budget, should fail with an error whose message names the locked path. Given that file then closed, a retried request should succeed.

The daemon side is `pu_core::fs_retry` plus `handle_delete_worktree`: it waits on each killed agent's exit watch (up to 5 s), runs `git worktree remove --force`, and falls back to `fs_retry::remove_dir_all`. When a file stays locked, the reply is `delete_worktree_result` with `directory_removed: false` and an `error` that names the file and says it is locked, and the manifest keeps the worktree with status `failed`. The app re-creates its watchers on that reply, and the user can retry once the file is closed. Releasing watchers is the app's job (WIN-5.6), because the daemon cannot close another process's handles.

### Signals, hooks and introspection

- REQ-WINAPP-047: Given an agent launch, should bind a per-launch pipe `purepoint-<hash16>-signal-<nonce>` with the pipe SDDL and client SID check, export it in `PU_SIGNAL_SOCKET` with `PU_SIGNAL_TOKEN`, and accept one frame per connection (at most 256 KiB, a 1 s read timeout, the token compared in constant time).
- REQ-WINAPP-048: Given `pu`'s signal helper on Windows, should write the payload to the pipe and exit with its Unix exit codes. A watchdog thread replaces `SIGALRM` and ends the helper at its deadline.
- REQ-WINAPP-049: Given the Claude hook installer on Windows, should write hooks in exec form, `{"type":"command","command":"<pu.exe path>","args":[…]}`, with a test for a `pu.exe` path containing spaces.
- REQ-WINAPP-050: Given a Claude session registry row on Windows, should match the agent when the row's `pidDomain` equals `"win32:" + lowercase(COMPUTERNAME)`. It should use `procStart` against `GetProcessTimes` as the pid-reuse check.
- REQ-WINAPP-051: Given process introspection (`proc_process_tree`, `process_command`, Codex rollout discovery), should use Toolhelp32 or `sysinfo`, never `ps` or `lsof`. Codex rollouts fall back to the newest rollout written after the agent's launch.
- REQ-WINAPP-052: Given a channel message from a human on Windows, should author it as `human:<user SID>`.

### Remote machines

- REQ-WINAPP-053: Given a daemon connection to a machine, should start `ssh.exe` (found with `which` under `System32\OpenSSH`, or `PU_SSH`) per the relay command in Interfaces, and frame NDJSON over its stdio exactly as over the pipe.
- REQ-WINAPP-054: Given a relay that exits 69, should show the machine as daemon-stopped and offer Start daemon. Given exit 127 or 9009 (`pu` not found), should report that PurePoint must be installed on that machine. It should never start a daemon automatically (MACH-003).
- REQ-WINAPP-055: Given `pu --machine <name>` on Windows, should use the same relay, with no forward socket, `forward.lock` or stale-forward sweep compiled in.
- REQ-WINAPP-056: Given the machine registry, should read and write `%USERPROFILE%\.pu\machines.json` with the `machines.md` schema, shared by the app and the CLI. Writes are private atomic replaces under an `fs4` lock on `machines.json.lock`.

### App architecture

- REQ-WINAPP-057: Given `apps/purepoint-windows`, should hold `src-tauri` (its own Cargo workspace, excluded from the root `Cargo.toml`) and `ui` (Vite, React 19, TypeScript strict, ESLint, Prettier, Vitest), laid out as in Interfaces.
- REQ-WINAPP-058: Given `npm run gen:protocol`, should regenerate `ui/src/protocol/generated/*.ts` from `pu-core` with the `ts` feature. CI should fail when the committed files differ. Hex byte fields are typed `string`.
- REQ-WINAPP-059: Given the backend, should talk to the daemon only through `pu-transport` (local) or the ssh relay (remote). One-shot requests use their own connection with a 30 s timeout. Attach, status, grid, projects and fs streams each hold a dedicated connection, as the Swift services do.
- REQ-WINAPP-060: Given attach output, should send raw bytes to the UI over a Tauri `Channel` (`InvokeResponseBody::Raw`), never as JSON number arrays. A 50 MB stream should produce no main-thread task over 50 ms, and a byte-exact checksum.
- REQ-WINAPP-061: Given a terminal pane, should render xterm.js with the WebGL renderer (canvas fallback), the theme tokens, bracketed paste, SGR mouse, OSC 8 links, IME composition, `resize` on pane resize and `claim_size` on focus. Hidden panes are evicted 30 s after hiding.
- REQ-WINAPP-062: Given each Swift `@Observable` state class the parity matrix maps, should have exactly one Zustand store with the same fields and actions, and the platform-neutral Swift tests ported to Vitest.
- REQ-WINAPP-063: Given the app launching, should mirror `DaemonLifecycle`. It finds `pu-engine.exe` (bundled sidecar, then `%USERPROFILE%\.pu\bin`), reuses a healthy daemon of the same protocol version, otherwise starts `--managed --parent-pid <app pid>`, and restarts a daemon older than its binary. Given a version mismatch, should show the Mac's banner text.
- REQ-WINAPP-064: Given project files, should watch the manifest, worktrees, file trees and the machine registry with debounced `notify` watchers raising the same triggers as the Swift watchers. An atomic-rename write burst should fire exactly one refresh.
- REQ-WINAPP-065: Given WebView2, should disable browser accelerator keys and, in release builds, the default context menu and devtools. Ctrl+R, F5, Ctrl+P, Ctrl+F, Ctrl+W, Ctrl+N, Ctrl+Plus/Minus, Ctrl+wheel, F12 and Ctrl+Shift+I never reload, print, find, zoom or open devtools.
- REQ-WINAPP-066: Given the Tauri window, should set `dragDropEnabled: false`, so HTML5 drag and drop works for tabs, panes and the sidebar.

### Hotkeys, fonts, chrome and icons

- REQ-WINAPP-067: Given the 32 `HotkeyAction`s, should ship exactly the default table in Interfaces, matched on `KeyboardEvent.code`, with no duplicate chord and no Ctrl+Alt chord (tests).
- REQ-WINAPP-068: Given a focused terminal, should pass every chord that is not an app binding through to the PTY. Ctrl+C, Ctrl+D, Ctrl+R, Ctrl+L and Ctrl+Z are never captured. Ctrl+C copies only while text is selected. Ctrl+Shift+C copies, and Ctrl+Shift+V pastes.
- REQ-WINAPP-069: Given custom bindings, should persist them per user in app data, reject reserved terminal chords and duplicates in the recorder, and apply changes live.
- REQ-WINAPP-070: Given terminals and the editor, should use `Cascadia Mono` 13 px with the Interfaces fallback stack. UI text uses Segoe UI Variable. Font size remains configurable as on the Mac.
- REQ-WINAPP-071: Given the custom title bar, should keep Windows 11 behaviour: Snap Layouts on hovering maximize, resizing from every edge and corner, maximize on a title-bar double-click, Win+Arrow snapping, and the light and dark caption-button states.
- REQ-WINAPP-072: Given every SF Symbol in the parity matrix, should map it to a Lucide component in `ui/src/theme/icons.ts`. A test fails on an unmapped name.
- REQ-WINAPP-073: Given the Mac theme files, should port every colour, font size and spacing token to CSS variables for light and dark, switching live with the system or the Settings override.

### Platform services

- REQ-WINAPP-074: Given Open Project, should use the Tauri dialog plugin's folder picker. Reveal and open-in-default-app should use the opener plugin (Explorer, `ShellExecute`).
- REQ-WINAPP-075: Given the Point Guard pairing secret, should store it in Windows Credential Manager through the `keyring` crate, under service `PurePoint.PointGuard.Pairing` with the endpoint as account, and never in a file.
- REQ-WINAPP-076: Given a new release, should check the signed `latest.json` feed with `tauri-plugin-updater`, verify the update signature with the public key in the app config, and offer the Mac's Check for Updates menu item and About status.
- REQ-WINAPP-077: Given the installer, should be an NSIS per-user install (no admin) that bundles `pu.exe`, `pu-engine.exe`, the Point Guard runtime and the WebView2 bootstrapper. Uninstall removes the app and the Run value, and leaves `%USERPROFILE%\.pu` intact.
- REQ-WINAPP-078: Given a waiting or broken agent while the window is unfocused, should post a Windows toast that focuses the agent's pane when clicked, and show the unseen count as a taskbar overlay badge.

### Unix unchanged and exclusions

- REQ-WINAPP-079: Given Linux and macOS, should build and pass the existing test suites unchanged, with `PROTOCOL_VERSION` still 14 and every Windows code path `cfg(windows)`.
- REQ-WINAPP-080: Given the Exclusions list, should not port any listed file or feature, and should provide the listed replacement wherever the Mac exposes the feature.

## Sum Sheet

Windows app = Mac app on Windows 11 22H2+, with a native daemon. Endpoint = private pipe named by hash(normalized socket path). Client trusts server ⇐ SID match ∧ PID = live `<socket>.pid`. Agent = ConPTY + suspended spawn + kill-on-close job, with the desktop-app policy set. Stop = agent keystrokes → `ClosePseudoConsole` → `TerminateJobObject`. Shim forwarder ⇒ launch the target exe, else `cmd /d /s /c`. Environment = daemon env + HKCU/system PATH − `NO_COLOR`. Private = protected DACL (user and SYSTEM full, Administrators read), reparse points refused. Single instance = `fs4` lock. Autostart = HKCU Run. Install = `%USERPROFILE%\.pu\bin`. Paths: no `\\?\`, case-insensitive compare. Worktree delete = watchers released → job empty → retried remove → locked path named. Remote = `ssh.exe … pu daemon proxy` per connection. UI = Tauri 2 + React/TS + xterm.js + Zustand + ts-rs types. Unix and protocol 14 unchanged.

## Interfaces

### Daemon endpoint (implemented in `pu-transport`)

| Item | Value |
|---|---|
| Socket path (stem) | `%USERPROFILE%\.pu\daemon.sock`, or `--socket <path>`. Never created on Windows |
| Normalization | create the parent directory; `dunce::canonicalize` the parent; append the file name; `/` → `\`; uppercase the drive letter; strip a trailing `\`; **no lowercasing** |
| Pipe name | `\\.\pipe\purepoint-<hex of SHA-256(normalized UTF-8)[0..8]>-daemon` |
| Other roles | same hash, `-signal-<nonce>` (per-launch signal pipe) |
| Pipe SDDL | `O:<user SID>D:P(A;;GA;;;<user SID>)(A;;GA;;;SY)` (reads back as `FA`) |
| Flags | first instance `FILE_FLAG_FIRST_PIPE_INSTANCE`; every instance `PIPE_REJECT_REMOTE_CLIENTS`. A failed first create (error 5) means `AddrInUse` |
| Server check of client | `GetNamedPipeClientProcessId` → `OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION)` → `TokenUser` must equal the daemon's SID |
| Client check of server | `GetNamedPipeServerProcessId` → token user SID must equal the caller's; and if `<socket>.pid` names a live process, it must be the server PID |
| Busy | client retries `ERROR_PIPE_BUSY` for 2 s. Over-cap connections get `BUSY` through an awaited `write_all` (1 s timeout, 128 concurrent writers at most) |
| Disconnect probe | `PeekNamedPipe` (broken pipe) plus a tokio readiness check into the connection's stash (`ReadHalf::peer_sent_or_closed`) |
| Half-close | none; a connection ends when both halves drop |
| Siblings | `daemon.sock.pid`, `daemon.sock.lock`, `daemon.sock.projects.json`, `daemon.log` in the same directory |

### PTY host contract (`pty_manager`, Windows)

Spawn sequence:

1. Create two anonymous pipes (input and output).
2. `CreatePseudoConsole(COORD{cols, rows}, in_read, out_write, 0, &hpc)`. No passthrough flag.
3. Build the attribute list: `PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE = hpc` (the HPCON value itself), and `PROC_THREAD_ATTRIBUTE_DESKTOP_APP_POLICY = PROCESS_CREATION_DESKTOP_APP_BREAKAWAY_DISABLE_PROCESS_TREE` (2).
4. Fill `STARTUPINFOEXW` with `STARTF_USESTDHANDLES` and null std handles.
5. Call `CreateProcessW(NULL, cmdline, …, bInheritHandles = FALSE, EXTENDED_STARTUPINFO_PRESENT | CREATE_SUSPENDED | CREATE_UNICODE_ENVIRONMENT, env block, cwd)`.
6. Close `in_read` and `out_write`.
7. `CreateJobObjectW`, then set `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`, then `AssignProcessToJobObject`, then `ResumeThread`. If any step fails, terminate the suspended process and fail the spawn.
8. Start a dedicated reader thread on `out_read`. It runs until EOF.

`PtyHandle` on Windows holds the HPCON, the input pipe writer, the process and job handles, the pid and the last size set. `size()` returns that size. `resize(cols, rows)` calls `ResizePseudoConsole`, records the size and resizes the parser in order with the output stream.

Command line per launch form:

| Resolved command | Launched as |
|---|---|
| `.exe` | `"<exe>" <args>` with MSVC CRT quoting |
| `.cmd`/`.bat` that only forwards (`@"<exe>" %*`, optionally `@echo off` and `setlocal`/`endlocal`) to an existing `.exe` | that exe, as above |
| any other `.cmd`/`.bat` | `cmd.exe /d /s /c "<"shim"> <args>"`. An argument is quoted if it is empty or contains a space, tab or any of `"&|<>^()%!,;=`. Inside the quotes `"` becomes `""` and `%` becomes `%%cd:~,%`, backslash runs before a quote are doubled, and newline or NUL is rejected |
| `shell` sentinel / `terminal` agent | first found of `pwsh.exe`, `powershell.exe`, `%COMSPEC%` |

Stop sequence (`kill(grace)`, also used by `kill_all_sessions`, `Engine::drop` and `stop_process_group`):

| Step | Claude | Codex | Other agents and shells |
|---|---|---|---|
| 1. Keystrokes (at most 1 s) | write `\x03`, wait 300 ms, write `\x03` | write `\x03` once | none |
| 2. | `ClosePseudoConsole`, then wait until the grace period ends | same | same |
| 3. | `TerminateJobObject(job, 137)` | same | same |
| 4. | `TerminateProcess` every descendant snapshotted (Toolhelp32) before step 1 that is still alive and not in the job, after checking its creation time | same | same |

Console control events (`GenerateConsoleCtrlEvent`) are never used.

Exit codes: `GetExitCodeProcess` as `i32` (`0xC000013A` is reported as `-1073741510`). A job kill reports 137, so the existing "killed" handling matches Unix's `128 + SIGKILL`.

### Agent environment

| Variable | Rule |
|---|---|
| base | the daemon's environment |
| `PATH` | `%USERPROFILE%\.pu\bin`; `%USERPROFILE%\.cargo\bin`; `%APPDATA%\npm`; `%LOCALAPPDATA%\Programs`; `%USERPROFILE%\.local\bin`; then the current `HKCU\Environment` `PATH` and the system `PATH` from the registry (expanded); then the daemon's `PATH`. Joined with `std::env::join_paths`, with duplicates removed case-insensitively |
| removed | `NO_COLOR`, `CLAUDECODE`, `CLAUDE_CODE_*`, `WT_*` |
| set | `TERM=xterm-256color`, `COLORTERM=truecolor`, `PU_SIGNAL_SOCKET` (signal pipe name), `PU_SIGNAL_TOKEN`, plus the Unix `PU_*` variables |
| user name | `USERNAME`, or `GetUserNameW` |
| home | `HOME` when set (test isolation), else `USERPROFILE` (`pu_core::paths::home_dir`) |

### Private files (`pu-core::private_fs`)

| Object | SDDL |
|---|---|
| file | `D:P(A;;FA;;;<user SID>)(A;;FA;;;SY)(A;;GR;;;BA)` |
| directory | `D:P(A;OICI;FA;;;<user SID>)(A;OICI;FA;;;SY)(A;OICI;GR;;;BA)` |

Owner: the current user. `check_private` accepts the user, SYSTEM or Administrators as owner, and only those three as allow entries.

### Autostart (`service.rs`, Windows manager)

| Item | Value |
|---|---|
| Key | `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` |
| Value name | `PurePoint Engine` |
| Data | `"%USERPROFILE%\.pu\bin\pu-engine.exe"` (expanded, quoted) |
| Scope | default socket only, as on Unix |
| Status | installed when the value exists; `StartupApproved\Run` disabled ⇒ reported as "disabled in Startup apps" |
| Optional | COM `TASK_TRIGGER_LOGON` task with `UserId = DOMAIN\user`, `LogonType = InteractiveToken`, `RunLevel = LUA`, used only if restart-on-failure is needed |

### Install layout

| Path | Contents |
|---|---|
| `%USERPROFILE%\.pu\bin\pu.exe`, `pu-engine.exe` | CLI and daemon (installer, `install.ps1`, app first launch) |
| `%USERPROFILE%\.pu\` | `daemon.sock.{pid,lock,projects.json}`, `daemon.log`, `machines.json`, `attachments\`, the same files as on Unix |
| `<project>\.pu\` | manifest, `workspaces.json` v4 (byte-compatible with the Mac), worktrees |
| Tauri app data dir | window geometry, sidebar width, selection, open projects, preferences, custom key bindings |
| Credential Manager | `PurePoint.PointGuard.Pairing` / `<endpoint>` |
| `%LOCALAPPDATA%\PurePoint\point-guard` | Point Guard bridge state (WIN-9.1) |
| app install dir (NSIS per-user) | app exe, `pu.exe`/`pu-engine.exe` sidecars, Point Guard runtime |

### Remote relay (Windows client)

```
ssh.exe -T -o BatchMode=yes -o ServerAliveInterval=15 -o ServerAliveCountMax=3 <ssh_host> <relay>
```

- Unix machine: `<relay>` is `/bin/sh -c 'PATH="$HOME/.pu/bin:$HOME/.cargo/bin:$PATH"; export PATH; exec pu daemon proxy'`, the same as the iPad.
- Windows machine (detected once per connection by the start probe: `uname -s` exits non-zero or prints nothing): `<relay>` is `pu daemon proxy`. Windows sshd builds the session environment from the user's registry `PATH`, which includes `%USERPROFILE%\.pu\bin`.
- Exit 0 means a normal close, 69 means no daemon, and 127 or 9009 means `pu` is missing.
- Start daemon runs the machine's start command (`machines.md`, or `pu health` on a Windows machine) with a 60 s limit.

### App layout (`apps/purepoint-windows`)

```
src-tauri/   Cargo workspace (excluded from root), deps: pu-core (feature ts), pu-transport, tauri 2,
             tauri-plugin-{dialog,opener,notification,updater}, notify, keyring
  src/daemon         one-shot client + lifecycle (DaemonClient, DaemonLifecycle)
  src/attach         per-pane attach connections → Tauri Channel (raw bytes)
  src/subscriptions  status, grid, projects, fs streams → typed events
  src/watchers       manifest, worktree, file tree, machine registry (notify)
  src/git            git/gh for local projects
  src/conversations  Claude, Codex, OpenCode indexes + transcript parser
  src/secrets        Credential Manager
  src/installer      CLI install into %USERPROFILE%\.pu\bin
ui/src/
  protocol/generated ts-rs output (committed, drift-checked)
  model/ state/ services/ views/ theme/   mirror the Swift folders
```

Tauri surface (typed wrappers in `ui/src/services`): commands `daemon_request(json)`, `daemon_health()`, `attach_open/attach_input/attach_resize/attach_claim_size/attach_close`. Events: `status_event`, `grid_event`, `projects_event`, `fs_event`, `daemon_lost`.

### Hotkey defaults (DEC-8)

Chords match on `KeyboardEvent.code`. Every binding can be remapped.

| Category | Action | Mac | Windows |
|---|---|---|---|
| application | newAgent | ⌘N | Ctrl+Shift+N |
| application | openProject | ⌘O | Ctrl+Shift+O |
| application | settings | ⌘, | Ctrl+, |
| navigation | focusSidebar | ⌘[ | Ctrl+Shift+[ |
| navigation | focusContent | ⌘] | Ctrl+Shift+] |
| navigation | toggleSidebar | ⌘\ | Ctrl+Shift+\ |
| navigation | toggleWorkspaceSidebar | ⌥⌘\ | Alt+Shift+\ |
| navigation | navDashboard | ⌃1 | Alt+Shift+1 |
| navigation | navAgents | ⌃2 | Alt+Shift+2 |
| navigation | navSchedule | ⌃3 | Alt+Shift+3 |
| panes | splitBelow | ⌘D | Alt+Shift+- |
| panes | splitRight | ⇧⌘D | Alt+Shift+= |
| panes | closePane | ⇧⌘W | Alt+Shift+W |
| panes | focusUp | ⌥⌘↑ | Alt+↑ |
| panes | focusDown | ⌥⌘↓ | Alt+↓ |
| panes | focusLeft | ⌥⌘← | Alt+← |
| panes | focusRight | ⌥⌘→ | Alt+→ |
| tabs | closeAgent | ⌘W | Ctrl+Shift+W |
| tabs | newTab | ⌘T | Ctrl+Shift+T |
| tabs | nextTab | ⇧⌘] | Ctrl+Tab |
| tabs | previousTab | ⇧⌘[ | Ctrl+Shift+Tab |
| tabs | selectTab1 … selectTab8 | ⌘1 … ⌘8 | Ctrl+Shift+1 … Ctrl+Shift+8 |
| tabs | selectLastTab | ⌘9 | Ctrl+Shift+9 |
| tabs | breakTab | ⌥⌘D | Alt+Shift+D |
| chat | toggleChatSidebar | ⇧⌘S | Ctrl+Shift+S |

Terminal clipboard: Ctrl+Shift+C copies and Ctrl+Shift+V pastes. Ctrl+C copies when text is selected, and is otherwise sent to the PTY.

### Fonts, chrome and icons

| Item | Value |
|---|---|
| Terminal and editor font | `"Cascadia Mono", Consolas, monospace`, 13 px default |
| UI font | `"Segoe UI Variable Text", "Segoe UI Variable", "Segoe UI", sans-serif` (Display optical size for titles) |
| Window | `decorations: false`; `data-tauri-drag-region` title area; caption buttons with Segoe Fluent Icons glyphs (minimize, maximize/restore, close); the backend answers `WM_NCHITTEST` with `HTMAXBUTTON` over maximize |
| Icons | `lucide-react`, one map in `ui/src/theme/icons.ts` generated from the parity matrix SF Symbols table; `.fill` symbols use the same icon with `fill="currentColor"` |

## Edge Cases

- **Store pwsh as the agent shell.** Its children leave the job unless the desktop-app policy attribute is set (WIN-1.1 run B). The attribute is mandatory, and the descendant sweep covers anything that still escapes.
- **Agent mid-turn at kill.** `\x03` interrupts the turn instead of exiting. The keystroke step is capped at 1 s, and `ClosePseudoConsole` then stops it. That exit is not clean (Claude 129), but suspended agents still resume from their recorded session.
- **Codex behind a `.cmd` shim that is not a pure forwarder.** cmd stays in the tree, so only one `\x03` is ever sent, and step 2 ends cmd.
- **`NO_COLOR` set system-wide.** It is removed from agent environments only. The CLI's own output still honours it.
- **Daemon started inside a kill-on-close job** (an IDE terminal, CI). The CLI spawns it with `CREATE_BREAKAWAY_FROM_JOB` when the job allows breakaway. Otherwise the daemon lives only as long as that job, and `pu` warns once in `daemon.log`.
- **Same-user impostor on the pipe name.** If it binds first, the real daemon's first-instance create fails (`AddrInUse`) and it exits with a message. Clients refuse the impostor because its PID differs from the live PID in `daemon.sock.pid`. With no PID file, clients connect (the SID matched). This narrows the attack but is not a boundary against code already running as the user.
- **Stale `daemon.sock.pid` naming a recycled PID.** Clients compare against a live process only, and the daemon rewrites the file after taking the lock, so a stale file never blocks a new daemon.
- **Logoff or shutdown.** The GUI-subsystem daemon may be terminated without a control event. Agent trees die through `KILL_ON_JOB_CLOSE`, and the next daemon's init marks them suspended so they resume, as after any crash.
- **Windows Defender or the search indexer holding a file in a worktree.** `fs_retry` waits it out. Past 2 s the error names the path, and `docs/guide/windows.md` documents adding Defender exclusions for `.pu\worktrees`.
- **Editor tab open on a file in a worktree being deleted.** WebView2 holds no OS handle on files the app reads through the backend, so only watchers need releasing. Unsaved edits trigger the Mac's close confirmation first.
- **Two folders that differ only by case** in a case-sensitive directory (`fsutil file setCaseSensitiveInfo`). Pipe names stay distinct (no lowercasing). Project identity compares case-insensitively, so such twins are treated as one project. This is documented as unsupported.
- **`HOME` set by Git Bash or MSYS.** `home_dir` prefers `HOME` for test isolation. Git Bash sets it to the same profile directory in MSYS form, which `dunce` normalizes to the same pipe name.
- **Keyboard layouts.** Chords match physical `code`s, so AZERTY's digit row and AltGr characters do not change app chords. A bare Alt+Shift press, which is the input-language switch, is not an app chord.
- **Replacing a running `pu-engine.exe`.** The image is locked, so the installer renames it aside (REQ-WINAPP-037). `install.ps1` and the updater do the same.
- **Remote Windows machine whose sshd job ends the session's processes.** A daemon started by Start daemon may die with the SSH session. The start probe reports it as stopped, and the guide recommends `pu daemon install` on that machine.
- **Many panes on a remote machine.** Each daemon connection is its own `ssh.exe` and SSH handshake. Startup latency grows with pane count, but Win32-OpenSSH has no `MaxSessions` overflow problem because no connection is shared.

## Exclusions

These Mac and iPad items are not ported. Each one's Windows replacement is listed beside it.

| Not ported | Windows replacement |
|---|---|
| Legacy chat: `State/ChatState.swift`, `Views/Chat/{ChatAreaView,ChatInputView,ConversationSidebarView}.swift`, and `CodeBlockView`, `ContentBlockView`, `MarkdownTextView`, `MessageStreamView` (reachable only from `ChatAreaView`), plus `ChatStateTests` | None needed: unused in the Mac app (iPad DEC-9). Point Guard chat is ported separately (WIN-9.4 – 9.6), and session lists and stream accumulation are ported (WIN-8.16) |
| `Views/MockUI/*`, `Views/PresentedControlDefaults.swift` | None: unreferenced on the Mac |
| The `+iOS` files, `PurePointiPadApp.swift`, and the iOS-only tests | None: iPad build only. The Mac originals they twin are ported |
| Sparkle (`CheckForUpdatesViewModel` and its feed) | `tauri-plugin-updater` with a signed `latest.json` (REQ-WINAPP-076) |
| Keychain (`PiPairingSecret` and other Keychain items) | Windows Credential Manager through `keyring` (REQ-WINAPP-075) |
| `NSOpenPanel`, `NSWorkspace.open`, Reveal in Finder | Tauri dialog plugin folder picker, opener plugin (default app, Reveal in Explorer) (REQ-WINAPP-074) |
| Citadel SSH (`Services/SSH/*`: transport, identity store, host-key pins, key format, sessions) | system `ssh.exe` running `pu daemon proxy` (REQ-WINAPP-053). Keys, agent and `known_hosts` are the user's own OpenSSH setup, as on the Mac |
| `ssh -L` Unix-socket forward, `forward.lock`, stale-forward sweep | the same exec relay, one `ssh.exe` per connection (WINAPP-013) |
| `HotkeyMonitor` (`NSEvent` monitor) | a WebView `keydown` handler on `KeyboardEvent.code` (REQ-WINAPP-067) |
| Dock tile badge, `NSUserNotification`/`UNUserNotificationCenter` | taskbar overlay badge and toasts through `tauri-plugin-notification` (REQ-WINAPP-078) |
| `BundleSeal` (`SecStaticCodeCheckValidity`) | an integrity check of the installed Point Guard runtime before the bridge starts; WIN-9.3 picks the mechanism (Authenticode or the runtime manifest hashes) |
| launchd agent / systemd unit (`pu daemon install`) | HKCU `Run` value (WINAPP-011) |
