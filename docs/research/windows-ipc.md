# Windows named-pipe IPC spike (WIN-1.2)

**Researched: 2026-11-25** on Windows 11 Home 26H2, **build 10.0.26300.9974**, unelevated (`whoami /groups` shows `Mandatory Label\Medium Mandatory Level`), Rust 1.89.0 MSVC, tokio 1.54.1, windows-sys 0.61, Node v26.13.1.

Code: `spikes/pipes` (its own `[workspace]`, not a root member). To reproduce, run `cd spikes/pipes && cargo test -- --nocapture`. All 16 tests pass, and they passed on repeated parallel runs.

## Question

Can Windows named pipes replace the daemon's Unix socket (`~/.pu/daemon.sock`) while keeping its properties? Those properties are:

- private to the user;
- a peer check;
- 1 MB NDJSON frames and a 30 s first-request timeout;
- 1024 connections, with one `BUSY` line for any connection after that;
- `MSG_PEEK` disconnect detection;
- Node clients.

The spike also covers the agent-signal `UnixDatagram`.

## Outcome

| # | Criterion | Result | Evidence (test) |
|---|---|---|---|
| 1 | Private DACL, first instance | **PASS.** The pipe reads back as `O:<me>D:P(A;;FA;;;<me>)(A;;FA;;;SY)`. A second `FILE_FLAG_FIRST_PIPE_INSTANCE` create fails with os error 5. A pipe that grants only SY refuses this user with os error 5, so the DACL is enforced. | `ipc::given_private_pipe_should_grant_only_user_and_system_and_own_the_name` |
| 2 | Client PID and token SID check | **PASS.** `GetNamedPipeClientProcessId`, then `OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION)`, then `TokenUser`. The client's PID equals the test PID. When the expected SID is injected as `S-1-5-18`, the client is rejected: it gets EOF and no reply, and `rejected_sid=1`. | `ipc::given_same_user_client_…`, `ipc::given_client_token_sid_differs_should_reject` |
| 3 | Disconnect detected mid-operation with no payload read | **PASS.** A zero-length `PeekNamedPipe` fails with `ERROR_BROKEN_PIPE`. The handler polls every 10 ms and saw the drop **31 ms** after the client closed. | `ipc::given_client_vanishes_mid_operation_should_detect_via_peek` |
| 4 | 1100 concurrent clients | **PASS** (3 of 3 runs): 1024 accepted, 76 `BUSY`, 0 other, in about 1.6 s. This needed a fix: see Findings 1. | `ipc::given_1100_concurrent_clients_…` |
| 5 | Node client | **PASS.** `net.createConnection('\\\\.\\pipe\\…')` round-trips `{"type":"health"}` and gets back `health_report` with the server's PID. | `ipc::given_node_client_should_round_trip_health`, `node/health.mjs` |
| 6 | Per-launch, token-guarded signal pipe | **PASS.** The Rust helper (a separate `pipes-spike signal-send` process) and the Node `node/signal-writer.mjs` each deliver one frame per connection. A wrong token is rejected. 32 concurrent writers each arrive exactly once. | `signal::*` |
| 7 | Client-side server verification | **PASS.** `GetNamedPipeServerProcessId`, then the server token's SID must equal the caller's. When the expected SID is injected as `S-1-5-19`, the client refuses (`Refusal::Sid`). A **real same-user impostor process** that squats the name passes the SID check. It is refused by the PID check (`daemon.sock.pid`) and by the image-path check. | `verify::*` |
| 8 | Unelevated autostart | `schtasks /SC ONLOGON` is **denied**. A per-user logon trigger through the COM or ScheduledTasks API **works**. The HKCU `Run` key **works**. All of them were cleaned up. | See Autostart below |
| 9 | DEC-2 naming | **PASS.** Two temporary HOMEs give different names. Eight spellings of one home give a single name: upper, lower, `/`, trailing `\`, `\\?\`, `sub\..`, a junction and the 8.3 short name. | `naming::*` |

## Findings

1. **A BUSY line written with `try_write` is lost for some clients.** A freshly connected `NamedPipeServer` has no write readiness yet, so `try_write` returned `WouldBlock` for 6–9 of 76 over-cap clients. The Unix daemon's synchronous `reject_busy` therefore does not port directly. The spike's fix is to spawn an awaited `write_all` with a 1 s timeout, bounded by its own 128-permit semaphore. Over-cap clients beyond those permits are closed without a line. After the fix, 76 of 76 clients got `BUSY` in every run. When the write completes, dropping the server handle does not discard the line.
2. **Keep at least one instance at all times.** Create the next listening instance *before* dropping or handing off the connected one. If no instance exists, clients get `ERROR_FILE_NOT_FOUND` instead of the retryable `ERROR_PIPE_BUSY`. Clients retry `PIPE_BUSY`: the spike sleeps 2 ms in a loop, and the sync helper uses `WaitNamedPipeW`.
3. **PeekNamedPipe does not see bytes that tokio or mio already prefetched.** mio posts the next overlapped read as soon as it has delivered data. Bytes sent during an operation were therefore already taken by mio's internal buffer, and `PeekNamedPipe` reported `Idle`, not `Pending`. On Unix, "unread bytes mean the client moved on" (`reader.buffer()` or `MSG_PEEK` sees the data). On Windows that check needs `PeekNamedPipe` for broken-pipe detection plus a tokio-level readiness check, such as polling `poll_read_ready` or `try_read` into the connection's buffer. Disconnect detection (criterion 3) is unaffected: the outstanding read does not hide `ERROR_BROKEN_PIPE`.
4. `reject_remote_clients(true)` (`PIPE_REJECT_REMOTE_CLIENTS`) is set, so SMB clients cannot connect. The pipe has no mandatory label, so it gets the implicit Medium no-write-up label, and Low-integrity or sandboxed processes of the same user cannot write to it.
5. The client check uses the PID, which has a small PID-reuse window. `ImpersonateNamedPipeClient` plus `OpenThreadToken` checks the token of the connection itself and has no such race. That is worth considering in WIN-2 (the DACL already excludes other users, so this check is defence in depth).

## Recommendations for later leaves

**SDDL.** Use `O:<user-sid>D:P(A;;GA;;;<user-sid>)(A;;GA;;;SY)` (it reads back as `FA`). This gives a protected DACL with no inherited ACEs, Everyone, Administrators or network access. Create the first instance with `first_pipe_instance(true)` and `reject_remote_clients(true)`. If creation fails with error 5, someone owns the name: treat it as "daemon already running or name squatted" and verify the owner the way a client would.

**Naming (DEC-2).** `\\.\pipe\purepoint-<hash16>-daemon`, where `<hash16>` is the lowercase hex of the first 8 bytes of SHA-256 over the UTF-8 of the normalized `~/.pu` path. Normalization:

1. `create_dir_all(home\.pu)`;
2. `dunce::canonicalize`, which uses `GetFinalPathNameByHandleW`. It resolves junctions, symlinks, `..` and 8.3 names, and returns the **on-disk casing**. dunce strips `\\?\` when it is safe to;
3. change `/` to `\`, make an ASCII drive letter uppercase, and strip a trailing `\`;
4. **do not lowercase.** Step 2 already gives a stable casing, and lowercasing would merge distinct directories inside case-sensitive folders. Measured: every case variant of a home canonicalized to the same string.

Use the same hash for the other roles, for example `-signal-<nonce>`.

**Server verification (client side).**

1. Open the pipe.
2. Call `GetNamedPipeServerProcessId` and get that process's token user SID. It must equal the caller's, which stops other users and services that squat the name.
3. The PID must equal the PID in `~/.pu/daemon.sock.pid`, and `QueryFullProcessImageNameW` must match the expected `pu` binary path (canonical, case-insensitive). These two checks are what stop a **same-user impostor**, which the SID check cannot catch (proven by the impostor test).

Recommendation: do the SID and PID checks on every connect. Add the image check where the expected path is known (the CLI's own `current_exe`, or the daemon path the app bundles). If the PID file is missing or stale, refuse and fall back to the autostart path rather than trusting the pipe. A same-user attacker can still rewrite `daemon.sock.pid` or replace the binary, so this narrows the attack but is not a boundary against code that already runs as the user.

**Autostart.** Do not use `schtasks.exe /SC ONLOGON`: it is "Access is denied" even with `/RU <me> /IT`, because its logon trigger has no `UserId` and so applies to any user, which needs admin. What works unelevated:

- the Task Scheduler COM API (`Schedule.Service`), using a `TASK_TRIGGER_LOGON` trigger with `UserId = DOMAIN\user`, `LogonType = InteractiveToken` and `RunLevel = LUA`, registered in `\` (state Ready);
- the equivalent `Register-ScheduledTask -Trigger (New-ScheduledTaskTrigger -AtLogOn -User me)`, which also ran on demand with result 0;
- `HKCU\…\CurrentVersion\Run`, which can be written and deleted.

The same COM trigger without `UserId` is `E_ACCESSDENIED`. Creating a task folder through COM unelevated also succeeded, and the folder was deleted.

Recommendation: use the HKCU `Run` value as the default. It is the simplest, needs no COM, and the user can see it in Settings > Startup apps (the `StartupApproved` key). Use a per-user COM logon task only if restart-on-failure or a delay is needed.

**Signal pipe.** Use a per-launch name, `purepoint-<hash16>-signal-<random nonce>`, with the same SDDL and a client SID check. Pass `PU_SIGNAL_PIPE` and `PU_SIGNAL_TOKEN` in the environment. The protocol is one frame (`{"token","payload"}`) per connection, delimited by EOF, with a 256 KiB maximum and a 1 s read timeout. Compare the token in constant time. The Rust helper opens the pipe as a file, waits out `PIPE_BUSY` with `WaitNamedPipeW` until its deadline, writes and closes. The Windows helper needs a watchdog thread that calls `process::exit(0)` at 100 ms, because there is no `SIGALRM` or `setitimer`. The `.mjs` writers can keep spawning the helper, or write directly with `net.createConnection(pipe, () => s.end(frame))`; both are proven.

## Autostart raw results (unelevated, Medium IL)

| Mechanism | Result | Cleanup |
|---|---|---|
| `schtasks /Create /SC ONLOGON /TN PurePointSpike /TR "cmd /c exit" /RL LIMITED /F` | `ERROR: Access is denied.` (exit 1) | nothing created |
| same command plus `/RU DESKTOP-EXAMPLE\dev /IT` | `ERROR: Access is denied.` | nothing created |
| COM `RegisterTaskDefinition`, logon trigger with `UserId`, LUA, InteractiveToken | REGISTERED, state 3 (Ready) | `DeleteTask` OK |
| COM logon trigger with no `UserId` | `E_ACCESSDENIED` (0x80070005) | nothing created |
| `Register-ScheduledTask … -AtLogOn -User me`, run on demand | OK, LastTaskResult 0 | `Unregister-ScheduledTask` OK |
| `HKCU\Software\Microsoft\Windows\CurrentVersion\Run\PurePointSpike` | write, read and delete OK | value gone (verified) |

Afterwards, `Get-ScheduledTask 'PurePointSpike*'` returns 0 tasks, and the Run value and the COM test folder are absent.

## Not proven

- **Logon-time firing.** The registered logon trigger and the Run entry were not exercised by an actual logoff and logon. The task did run on demand.
- **Another real Windows user and a SYSTEM client** were not tested. Rejection of another user is shown by injecting a different expected SID, and DACL enforcement is shown by a pipe that grants only SY and refuses us.
- **Elevated daemon or client combinations.** The token SID is the same when elevated. The integrity level differs, and a Medium client opening a High-IL server pipe was not tested.
- **The 1 MB frame limit and the 30 s first-line timeout** use the same tokio code as on Unix (`take`/`read_until`, `timeout`). They are implemented in the spike but have no dedicated test.
