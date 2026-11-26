# Windows ConPTY host spike (WIN-1.1)

**Researched: 2026-11-25** on **Windows 11 Home, build 10.0.26300.9974** (`cmd /c ver`: `Microsoft Windows [Version 10.0.26300.9974]`), Rust 1.89 MSVC, `windows-sys` 0.61. The agent CLIs tested were Claude Code 2.1.332 (native `claude.exe`), codex-cli 0.172.0 (WinGet exe behind the `codex.cmd` shim), PowerShell 7 (Microsoft Store/MSIX build) and Windows PowerShell 5.1.

The spike code is in `spikes/conpty/` (`src/pty.rs` is the host and `src/main.rs` holds one subcommand per experiment). Parsed screens are in `spikes/conpty/fixtures/`. Raw byte dumps and run logs are in `spikes/conpty/results/`. User and host names in the outputs are redacted to `<user>` and `<host>`.

## Question

Can a Rust daemon host agent TUIs on Windows through ConPTY directly (DEC-1: no portable-pty)? That means it has to spawn them, render them through the vendored `vt100`, resize them, stop them gracefully and kill them with their whole process tree. What do later leaves need to know about launch forms, stop methods, the screen heuristics, the Claude registry and hooks?

## Outcome

| # | Check | Result |
|---|---|---|
| 1 | ConPTY + `CREATE_SUSPENDED` + job + resume | PASS. `spikes/conpty` has its own `[workspace]` table, so the root workspace ignores it and the root `Cargo.toml` needed no change. |
| 2 | First screens through vendored vt100 | PASS for pwsh, cmd, claude and codex (via its `.cmd` shim). |
| 3 | `.cmd` shim launch form and quoting | PASS. 8/8 quoting cases round-trip exactly. Naive `\"` escaping is injectable. |
| 4 | `ResizePseudoConsole` mid-session | PASS. Claude redraws at 70 columns and conhost re-wraps pwsh scrollback. |
| 5 | Zero processes after `TerminateJobObject` | PASS, **but only with `PROC_THREAD_ATTRIBUTE_DESKTOP_APP_POLICY`** when the shell is the MSIX (Store) pwsh. Without it, Store pwsh grandchildren escape the job. |
| 6 | Graceful stop of claude and codex | See the stop matrix below. |
| 7 | `PSEUDOCONSOLE_PASSTHROUGH_MODE` (0x8) | Accepted (S_OK), but makes no meaningful difference on this build. |
| 8 | Claude `pidDomain` on Windows | `"win32:" + lowercase(COMPUTERNAME)`, e.g. `win32:desktop-xxxxxxx`. |
| 9 | Hook runners | Claude: Git Bash by default, `shell:"powershell"` gives pwsh, and `args` gives a direct exec. Codex: `cmd.exe /C` (from source, not run). |

### 1. Host sequence (`spikes/conpty/src/pty.rs`)

1. Create two anonymous pipes with `CreatePipe`.
2. `CreatePseudoConsole(COORD{cols,rows}, in_read, out_write, flags, &hpc)`.
3. Build the proc-thread attribute list: `PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE` = `hpc` (the HPCON value itself, not a pointer to it), and `PROC_THREAD_ATTRIBUTE_DESKTOP_APP_POLICY` = `PROCESS_CREATION_DESKTOP_APP_BREAKAWAY_DISABLE_PROCESS_TREE` (2), which section 5 explains.
4. Fill `STARTUPINFOEXW` with `dwFlags = STARTF_USESTDHANDLES` and null std handles. Without this, a parent whose stdio is redirected leaks its pipes into the child.
5. `CreateProcessW(NULL, cmdline, …, bInheritHandles=FALSE, EXTENDED_STARTUPINFO_PRESENT | CREATE_SUSPENDED | CREATE_UNICODE_ENVIRONMENT, envblock, cwd)`.
6. Close `in_read` and `out_write`, because ConPTY holds its own copies.
7. `CreateJobObjectW`, then `SetInformationJobObject(JobObjectExtendedLimitInformation, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE)`, then `AssignProcessToJobObject`, then `ResumeThread`.
8. A dedicated thread drains `out_read` continuously. `ClosePseudoConsole` then returns in about 20–30 µs and never deadlocks.

The spike also passes `CREATE_BREAKAWAY_FROM_JOB`, because the spike itself runs inside an inherited job (limit flags `0x1800`, which is BREAKAWAY_OK plus SILENT_BREAKAWAY_OK). That flag turned out not to matter. Run E in `results/tree.txt` nests our job under the inherited one and still leaves zero survivors. A daemon can omit it.

**Environment (important for the heuristics).** The host shell exported `NO_COLOR=1`. With it, Claude draws no SGR at all: its dim `Try "…"` placeholder becomes plain text, `input_phase` reads `has_draft` on an empty box, and `deliver_prompt` would fail with `NotReady`. The daemon must remove `NO_COLOR`. It must also keep setting `TERM=xterm-256color` and `COLORTERM=truecolor` as `engine/pty_operations.rs` does, and drop `CLAUDECODE`, `CLAUDE_CODE_*` and `WT_*`. With that environment, the dim placeholder came through ConPTY with `ESC[2m…ESC[22m` intact (25 dim cells), and `input_phase` gave `input_ready`.

### 2. First screens (`fixtures/`)

| Fixture | Command | `input_phase` |
|---|---|---|
| `pwsh.txt` | `pwsh.exe -NoLogo` | busy (plain prompt) |
| `cmd.txt` | `cmd.exe` | busy |
| `claude.txt` | `claude.exe` in a trusted dir | **input_ready** (box `────` / `❯ Try "…"` / `────`) |
| `claude-untrusted-dir.txt` | `claude.exe` in a new dir | **dialog** (`trust this folder`, `Esc to cancel`) |
| `codex.txt` | `cmd.exe /d /s /c ""…\codex.cmd""` in this worktree | dialog (`Trust this folder?`) |
| `codex-trusted-override.txt` | the same, plus `-c "projects.'<dir>'.trust_level=""trusted"""` | busy (codex's `› Ask Codex…` composer has no borders, so the Claude box heuristic correctly does not match) |
| `claude-typed.txt` | typed text in the Claude box, Enter never sent | has_draft |

`results/typing.txt` is an end-to-end check of the `delivery.rs` input primitives through ConPTY, with no prompt submitted:

- 41 chars (quotes, non-ASCII, `❯`) written in 8-byte chunks appeared exactly in the box, and `squash` compared equal.
- `\x15` (Ctrl-U) emptied the box, and so did three `0x7f` bytes.
- `\x03\x03` exited the session with code 0.

### 3. `.cmd` / `.bat` shims

`codex.cmd` is a WinGet forwarder: `@"%LOCALAPPDATA%\…\codex-x86_64-pc-windows-msvc.exe" %*`. `CreateProcessW` cannot start a batch file through ConPTY without cmd, so the working launch form is:

```
cmd.exe /d /s /c "<"shim path"> <arg1> <arg2> …"
```

With `/s`, cmd strips only the outermost pair of quotes, so the inner text is used verbatim, and `/d` skips AutoRun. Arguments are encoded by `quote_cmd_arg` in `src/main.rs`, which was tested by `conpty-spike quoting` through `echo args.cmd`, a shim in `%TEMP%\pu spike dir\`. That shim forwards `%*` to an exe that prints its argv. The rules:

- Quote an argument if it is empty or contains any of: space, tab, `"`, `&`, `|`, `<`, `>`, `^`, `(`, `)`, `%`, `!`, `,`, `;`, `=`.
- Inside the quotes, write `"` as `""`. cmd's quote state toggles twice, so `&|<>` stay protected, and the MSVC CRT reads `""` inside a quoted argument as a literal `"`. A run of backslashes before a `"` or before the closing quote is doubled.
- Write `%` as `%%cd:~,%`, so `%PATH%` is not expanded on the `/c` command line. `100%`, `%PATH%` and `%USERNAME%x` all arrived literally.
- Newlines and NUL cannot pass. Reject them.
- Verified: `say "hi & bye"`, `x" & echo INJECTED & ` (arrived literally, nothing injected), `C:\dir with space\`, `bs\"q`, `bang!`, `tab\there`, empty argument, and a shim path with spaces.
- **Do not use CRT-style `\"`.** `"x\" & echo INJECTED & \""` ran `echo INJECTED`.

Section 6 adds a reason to prefer the target exe: when a simple forwarder shim (`@"<exe>" %*`) is resolved to that exe and launched directly, cmd is no longer in the tree.

### 4. Resize (`fixtures/*-resize-*`, `results/resize.txt`)

`ResizePseudoConsole(70x30)` returned S_OK. Claude redrew its whole UI at 70 columns: the border rules became 70 `─`, and the footer re-wrapped onto two rows. In the pwsh run, conhost re-wrapped 40 lines of 100 characters from one row each into two rows each and repainted the screen. To reproduce the screen exactly, the parser has to be resized at the moment of the resize (`screen_mut().set_size`) and must keep consuming the following bytes. Re-parsing the whole history at the new size is wrong. `delivery.rs::winsize` uses `TIOCGWINSZ`, which has no ConPTY equivalent, so the Windows host must track the size it last set.

### 5. Process tree teardown (`results/tree.txt`)

The root command was `pwsh -Command "Start-Process -NoNewWindow ping -t localhost; cmd /c ping -t 127.0.0.1"`, which gives a tree of pwsh → ping plus pwsh → cmd → ping.

- **A** (with the desktop-app policy): job ActiveProcesses was 4 before. After `TerminateJobObject`, the root exited in about 15 ms, ActiveProcesses was 0, and the Toolhelp32 snapshot showed **0 survivors** and no `PING.EXE` system-wide. Closing the job handle (`KILL_ON_JOB_CLOSE`) gave the same result. `ClosePseudoConsole` also ended the `conhost.exe` it had spawned.
- **B** (no policy, Store pwsh): ActiveProcesses was 1 and **all 3 grandchildren survived** both `TerminateJobObject` and the job-handle close. `IsProcessInJob` was false for each of them. The cause is that MSIX-packaged processes create their children outside the "desktop app runtime environment" by default.
- **C, D** (no policy, root `cmd.exe` or `powershell.exe` 5.1): every descendant stayed in the job, and there were 0 survivors.
- **E** (policy on, no `CREATE_BREAKAWAY_FROM_JOB`, our job nested in the inherited one): 0 survivors.

Later leaves must always set `PROC_THREAD_ATTRIBUTE_DESKTOP_APP_POLICY = PROCESS_CREATION_DESKTOP_APP_BREAKAWAY_DISABLE_PROCESS_TREE`, and should keep a Toolhelp32 descendant sweep as a fallback.

### 6. Graceful stop (`results/stop.txt`)

Each run was a fresh launch at the idle input. For Claude the phase was `input_ready`. Codex ran with the trust override at its empty composer. A run passes if the root exits within 2 s.

| Method | claude.exe | codex via `cmd /d /s /c codex.cmd` | codex exe direct |
|---|---|---|---|
| write `\x03` once | **no**: shows the exit hint and stays up | yes, 0.20 s, exit 0 | yes, 0.16 s, exit 0 |
| write `\x03`, wait 300 ms, `\x03` | **yes, 0.68 s, exit 0** | **no**: codex quits, the 2nd ^C reaches cmd, and cmd stays alive (2 processes) | yes, 0.31 s, exit 0xC000013A |
| `GenerateConsoleCtrlEvent(CTRL_C_EVENT, 0)`, sent by a helper that runs `AttachConsole(pid)` | **yes, 0.62 s, exit 0** | **no**: the screen shows `^CTerminate batch job (Y/N)?` | yes, 0.04 s, 0xC000013A (killed by the default handler) |
| `ClosePseudoConsole` | yes, 0.67 s, exit 129 | yes, 0.02 s, 0xC000013A | yes, 0.03 s, 0xC000013A |

Recommendations:

- **Claude:** write `\x03\x03` about 300 ms apart, or use `GenerateConsoleCtrlEvent`. Both exit with code 0.
- **Codex:** write `\x03` once at an idle composer. Never use console control events or a double ^C while a `.cmd` wrapper is in the tree.
- **Universal fallback:** `ClosePseudoConsole` stopped everything within 2 s, though not with a clean exit code. Follow it with `TerminateJobObject`.

`GenerateConsoleCtrlEvent` needs a helper process, because attaching to the pseudoconsole would detach the daemon from its own console. Not tested: ^C while a turn is running, which would interrupt the turn rather than exit and would require real work.

### 7. Passthrough mode and the delivery heuristics (`results/passthrough.txt`, `*-conpty.raw.txt` vs `*-passthrough.raw.txt`)

`CreatePseudoConsole` returned S_OK for flags 0x0, 0x1 (INHERIT_CURSOR), 0x2, 0x4 and **0x8 (PASSTHROUGH_MODE)**. With 0x8, the cmd, claude and codex output was byte-for-byte the same sequences except for ordering. Both modes contain conhost-generated sequences (`ESC[1C` cursor-forward in place of single spaces, `ESC[100X` erase-character, `ESC[r;cH` jumps). No DSR or DA1 queries arrived in either mode, so the host's responder never fired. On this build, passthrough brings no benefit, and the plan should not depend on it.

ConPTY output compared with what the screen heuristics expect:

- **Preserved:** SGR 2 and 22 (dim), truecolor `38;2`, the `❯` marker, the U+00A0 after `❯`, `─`/`╭╰│` borders and the alternate screen (`?1049h`). `find_input_box`, `rows_without_dim`, `input_phase`, `echoes` and `squash` all worked unchanged on vt100 screens.
- **Changed:** runs of spaces become `ESC[nC` or `ESC[nX`, and full-width rows end without CRLF because they rely on deferred wrap. vt100 renders both correctly: skipped cells read as blank and `squash` ignores whitespace. Raw-byte matching would break, so the daemon must stay on parsed screens.
- **Added by ConPTY at start:** `ESC[?9001h` (a win32-input-mode request; we answer with plain VT input, which works), `ESC[?1004h` (focus events) and an OSC 0 title. Claude's own `ESC[>0q`, `ESC[?u` and `OSC 7501;?` queries pass through to the host unanswered, without visible harm. Codex emits `OSC 7501;state=working|idle:app=codex`, a possible state signal.
- **Input:** `\r`, `\x15`, `0x7f`, `\x03` and UTF-8 text written to the input pipe all behave as on a Unix PTY.

### 8. Claude session registry on Windows (`results/registry.txt`)

The registry is at `%USERPROFILE%\.claude\sessions\<pid>.json`, the same relative path as on macOS and Linux, and the row is removed on exit. The row Claude wrote for the spike-launched pid:

```json
{"pid":14520,"sessionId":"…","cwd":"C:\\Users\\<user>\\dev","startedAt":1795653828085,
 "procStart":"134401274260337323","version":"2.1.332","peerProtocol":1,
 "peerFeatures":["notify_idle","artifact_yield"],"kind":"interactive","entrypoint":"cli",
 "pidDomain":"win32:<host>","messagingSocketPath":"\\\\.\\pipe\\LOCAL\\cc-msg-…","status":"idle",…}
```

- **`pidDomain` = `"win32:" + lowercase hostname`.** On this machine, `COMPUTERNAME=DESKTOP-XXXXXXX` (redacted here) gave `win32:desktop-xxxxxxx`, and the spike checked that the two match exactly. `COMPUTERNAME`, `hostname` and `Dns.GetHostName()` all agree here, so which of them Claude reads is not established.
- `procStart` is the decimal process-creation FILETIME and equals `GetProcessTimes` exactly. It can make the Windows pid-reuse check stronger than `startedAt`.
- `cwd` uses backslashes and the original case.
- `claude.rs::row` uses `O_NOFOLLOW`, uid and mode checks, which are Unix-only and need a Windows equivalent: reject reparse points and check the owner SID.

### 9. Hook runners and the command string for `install.rs`

Today `crates/pu-engine/src/signals/install.rs` writes no CLI hook commands. It only manages launch-owned artifacts under `.pu/agent-signals/<launch>`. Below is what a hook installer will need. The Claude probe used a per-launch `--settings` file with four SessionStart hooks that call a copy of the spike exe at `%TEMP%\pu spike dir\pu.exe`, a path with spaces:

| Hook form | Ran via | Argv received |
|---|---|---|
| `command: "\"C:/…/pu spike dir/pu.exe\" argv h1 \"$0\" \"x y\""` | **Git Bash**: `$0` = `C:/Program Files/Git/usr/bin/bash`, parent `bash.exe` | `h1, C:/Program Files/Git/usr/bin/bash, x y` |
| `command: "\"C:\\…\\pu spike dir\\pu.exe\" argv h2"` | Git Bash | `h2` (backslashes survive inside bash double quotes) |
| `shell: "powershell"`, `command: "& \"C:\\…\\pu.exe\" argv h3 \"x y\""` | `pwsh.exe` | `h3, x y` |
| `command: "C:\\…\\pu spike dir\\pu.exe"`, `args: ["argv","h4","arg with space","q\"uote"]` | **direct exec**, parent `claude.exe` | exact, including the embedded quote |

The Claude docs agree: the default shell is `bash`, or `powershell` on Windows when Git Bash is absent. `args` disables the shell.

The default shell is Git Bash or pwsh depending on the machine, and in pwsh a bare quoted path is a string, not a call, so no single shell-form string works on both. **`install.rs` must therefore write the exec form on Windows:**

```json
{"type":"command","command":"C:\\Program Files\\PurePoint\\pu.exe","args":["<subcommand>","<args>…"]}
```

If a shell form is ever required, pin `"shell":"bash"` and write `"\"C:/Program Files/PurePoint/pu.exe\" <subcommand>"` with forward slashes and the whole path double-quoted. A path containing `$`, `` ` ``, `"` or `\` immediately before those characters would need escaping.

**Codex.** The source (`codex-rs/hooks/src/engine/command_runner.rs`) runs hooks as `%COMSPEC%` (default `cmd.exe`) `/C` with the raw argument `"<command>"`, and its docs mention a `commandWindows` override. The matching string is `"\"C:\\Program Files\\PurePoint\\pu.exe\" <subcommand>"`. With the outer quotes Codex adds, cmd strips one pair and runs the quoted path. Arguments must follow the cmd rules in section 3. **This was not run empirically:** codex only runs hooks after the user trusts each hook's hash in the TUI, and the spike did not modify the user's codex trust config. OpenCode is integrated through a plugin path in its config, not a shell command, so shell quoting does not apply to it.

## Not proven / follow-ups

- Codex hooks were not run on Windows. The result comes from source only.
- Graceful stop while an agent is mid-turn was not tested, because it needs real work.
- Which hostname API Claude uses for `pidDomain` is unknown: all candidate APIs return the same value on this machine.
- Passthrough was tested only on 26300. Older builds (22H2/23H2) may behave differently. The design should not rely on it either way.
- The spike runs one experiment per process and hosts nothing long-lived. Reader-thread lifecycle, overlapped I/O and the daemon integration belong to later leaves.

## Reproduce

```pwsh
cd spikes\conpty; cargo build
$x = '.\target\debug\conpty-spike.exe'
& $x flags; & $x quoting; & $x tree
& $x screen claude C:\some\trusted\dir C:\Users\<user>\.local\bin\claude.exe 120 30 25
& $x screen codex . 'cmd.exe /d /s /c ""C:\Users\<user>\.local\bin\codex.cmd""' 120 30 25
& $x resize claude <dir> <claude.exe>; & $x stop claude <dir> <claude.exe> ctrlc-write-x2
& $x passthrough claude <dir> <claude.exe>; & $x typing <dir> <claude.exe>; & $x registry <dir> <claude.exe>
```
