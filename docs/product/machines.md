# Machines over SSH

**Maturity: SPECIFIED** | ID Prefix: MACH | Dependencies: `architecture/ipc-api.md`, `architecture/daemon-engine.md`, `architecture/desktop-app-integration.md`, `product/desktop-app.md`, `product/cli.md`

## Purpose

Let one PurePoint app (and the `pu` CLI) work with PurePoint daemons running on other computers the user can already reach over SSH. A remote machine is not a new kind of agent or a new execution protocol. It is another place where PurePoint already runs: its daemon owns its projects, worktrees, agents and processes, and the app simply shows and drives them the same way it shows local ones.

## Conceptual Model

- **Machine** = a user-chosen name plus an SSH destination (`~/.ssh/config` alias or `user@host[:port]`). Machine id is a UUID. `machineId == nil` means local.
- **Daemon endpoint** = a Unix socket path the app/CLI talks to. Local is `~/.pu/daemon.sock`. A machine's endpoint is a local socket that the system `ssh` forwards to `<remote $HOME>/.pu/daemon.sock`.
- **Project identity** = `(machineId, projectRoot)`. The same path on two machines is two projects.
- Everything a remote project does goes through its daemon endpoint: status, spawn, attach, worktrees, files, git.

```
PurePoint app / pu CLI
  ├── ~/.pu/daemon.sock                      → local daemon
  └── ~/.pu/machines/<id>/daemon.sock  ─ssh -L→ remote ~/.pu/daemon.sock → remote daemon
```

## Research Notes

**Researched: 2026-10-12.** The daemon protocol is newline-delimited JSON over a Unix stream socket with no client handshake beyond `Health` and no peer-credential check (`crates/pu-engine/src/ipc_server/mod.rs`). OpenSSH Unix-socket forwarding (`ssh -L local.sock:remote.sock`, `StreamLocalBindUnlink`) carries it unchanged; sshd opens the remote socket as the authenticated SSH user, so the remote socket's file permissions remain the authorization boundary. Source: [ssh(1) -L](https://man.openbsd.org/ssh#L), [ssh_config(5) StreamLocalBindUnlink, ControlMaster](https://man.openbsd.org/ssh_config).

The CLI auto-starts a local daemon when its socket does not answer (`daemon_ctrl::ensure_daemon`). Pointed at a forwarded path that would delete the forward and bind a local daemon in its place, so remote endpoints must never auto-start.

The macOS app reads project files, runs `git`/`gh`, watches files and persists workspace layouts directly on the local filesystem. For remote parity those operations need daemon requests.

Alternative considered: a dedicated mTLS worker protocol with enrollment, ledgers and guardians (PR #218, LAN mesh). Rejected for this scope: it creates a second execution protocol and a special "remote run" concept, while SSH already supplies identity, encryption and reachability and each daemon already owns its agents.

## Decisions

! [MACH-001] Transport is the system `/usr/bin/ssh` with Unix-socket forwarding and the user's own ssh config, agent and Keychain — PurePoint stores no keys or passwords.
! [MACH-002] The existing daemon protocol is reused unchanged over the forward — no second protocol; IPC-004 is answered for this transport by SSH authentication plus remote socket permissions.
! [MACH-003] Remote endpoints never auto-start a daemon. Starting a remote daemon is an explicit user action run over ssh on the remote machine.
! [MACH-004] The remote daemon owns its agents. Disconnecting, quitting the app or losing the network never stops remote work.
! [MACH-005] Project, workspace, selection and persisted-state identity is `(machineId, projectRoot)`; local keys keep their existing form.
! [MACH-006] The CLI targets a machine only through an explicit `--machine <name>` — agent ids can collide across daemons, so there is no implicit fan-out.
! [MACH-007] File and git access for any project goes through a project host abstraction; remote projects use the daemon file/git requests below, confined to registered project roots, their worktrees and agent transcript directories.
! [MACH-008] Machines are registered in `~/.pu/machines.json`, shared by the app and the CLI.

## Requirements

- Given a new machine with a reachable SSH alias and a running daemon, should list its registered projects, worktrees and agents under that machine in the sidebar.
- Given a reachable machine whose daemon is stopped, should show it as connected with the daemon stopped and offer Start daemon; should not start it automatically.
- Given a remote daemon with a different protocol version, should show the machine as incompatible and make no other requests.
- Given an unreachable machine, should show it offline and retry with backoff without blocking local projects.
- Given a remote agent, should attach, send input, resize and kill it exactly like a local agent.
- Given spawn or new worktree on a remote project, should run on that machine.
- Given a remote project, should browse, preview, edit and save files, show git status/diffs/review and persist workspace layouts on that machine.
- Given a save whose file changed remotely since it was read, should reject the write with a conflict.
- Given a file path outside the registered roots (including via `..` or symlinks), should refuse it.
- Given the same project path on two machines (or local and remote), should keep them as distinct projects.
- Given the app quits or the network drops, should leave remote agents running.
- Given `pu --machine <name> …` with the daemon down or the tunnel unavailable, should fail with an actionable error and never start a local daemon.

## Sum Sheet

Machine = SSH destination. Endpoint = forwarded socket. Identity = (machine, root). Daemon owns work. Files and git over IPC. CLI targets explicitly.

## Interfaces

### Machine registry `~/.pu/machines.json`

```json
{ "version": 1, "machines": [ { "id": "UUID", "name": "studio", "ssh_host": "studio-mac" } ] }
```

`name` is unique (case-insensitive) and is what `--machine` matches. The local forwarded socket for a machine is `~/.pu/machines/<id>/daemon.sock`; `~/.pu/machines` and each `<id>` directory are mode 0700.

### SSH forward

```
/usr/bin/ssh -N -T -o BatchMode=yes -o ExitOnForwardFailure=yes \
  -o ServerAliveInterval=15 -o ServerAliveCountMax=3 -o StreamLocalBindUnlink=yes \
  -o ControlMaster=no -o ControlPath=none \
  -L <local sock>:<remote home>/.pu/daemon.sock <ssh_host>
```

Remote home is resolved once per connection with `ssh -o BatchMode=yes -o ControlMaster=no -o ControlPath=none <ssh_host> 'printf %s "$HOME"'`. Start daemon runs `ssh -o ControlMaster=no -o ControlPath=none <ssh_host> '<start command>'`, where the start command (one argument) is exactly `/bin/sh -c 'PATH="$HOME/.pu/bin:$HOME/.cargo/bin:$PATH"; export PATH; exec pu health'`: `/bin/sh` makes it work whatever the remote login shell is (fish and csh can't parse the assignment), and the `PATH` prefix lets both `pu` and the `pu-engine` it spawns resolve (non-interactive SSH has a minimal `PATH`). It is given 60 s. The spawned daemon detaches into its own session with null stdio, so the SSH command returns and the daemon outlives it. `-o ControlMaster=no -o ControlPath=none` (on every ssh command) keeps a user's multiplexing config from routing a command through, or parking the forward on, a shared master that outlives it.

### Daemon file and git requests (protocol version 9)

All paths are absolute paths on the daemon host. Each is canonicalized and must lie inside a registered project root, one of its worktrees (a manifest worktree path counts only if it canonicalizes strictly inside `<root>/.pu/worktrees/`, and never `/` or an ancestor of the root or `$HOME`), or an allowlisted agent transcript directory (`~/.claude/projects`, `~/.codex/sessions`, `~/.local/share/opencode`; read-only: `fs_write` and `fs_delete` refuse them). Git metadata (any path with a `.git` component) is also read-only, so a write cannot plant repository config such as `diff.external` for a later `git_exec`. Otherwise the daemon answers `Error { code: "path_not_allowed" }`. Binary data is standard base64.

| Request (`type`) | Fields | Response (`type`) |
|---|---|---|
| `fs_list` | `path`, `recursive` (default false), `include_hidden` (default true), `max_entries` (default 20000, clamped to 100000) | `fs_list_result { path, entries: [{ name, path, is_dir, is_symlink, size, modified_ms, ignored }], truncated }` |
| `fs_stat` | `path` | `fs_stat_result { path, exists, is_dir, size, modified_ms }` |
| `fs_read` | `path`, `max_bytes` (default 1048576, clamped to 8 MiB) | `fs_read_result { path, data_base64, size, modified_ms, truncated }`; regular files only, else `Error { code: "not_a_file" }` (`"is_directory"` for a directory) |
| `fs_write` | `path`, `data_base64`, `expected_modified_ms` (optional), `create_dirs` (default false) | `fs_write_result { path, size, modified_ms }` or `Error { code: "conflict" }` |
| `fs_delete` | `path` | `ok`; a root or any ancestor of one (e.g. `<root>/.pu/worktrees`) is refused |
| `subscribe_fs` | `root`, `recursive` (default true) | `fs_subscribed`, then stream of `fs_event { root, paths: [String] }` (coalesced, ~200 ms). If a native recursive watch cannot be set up (e.g. the Linux inotify watch limit), the daemon polls the tree every 2 s instead |
| `git_exec` | `cwd`, `program` (`git` or `gh`), `args: [String]`, `stdin_base64` (optional), `timeout_ms` (default 30000, clamped to 120000) | `git_exec_result { exit_code, stdout_base64, stderr_base64, truncated }` (each stream capped at 8 MiB) |

`ignored` comes from `git check-ignore` relative to the repository containing `path` (given 10 s; on timeout nothing is reported ignored). `fs_write` creates its temp file with the existing file's mode, so a rewritten file is never more readable than before. `subscribe_fs` ends the stream when a client doesn't take an `fs_event` within 10 s. `Init` refuses `/`, `$HOME` and any ancestor of `$HOME` as a project root (`Error { code: "project_root_not_allowed" }`), since registering one would expose the whole home directory.

`git_exec` runs the program directly (no shell) with `cwd` confined like any other path, in its own process group (a timeout kills the whole group); `gh` uses the remote user's own auth. git runs with `GIT_CONFIG_GLOBAL=/dev/null`, `GIT_CONFIG_NOSYSTEM=1` and `-c core.fsmonitor= -c core.hooksPath=/dev/null` prepended to the arguments, so neither user/system config nor a repository's fsmonitor or hooks run helpers. Arguments outside what the app sends are refused with `Error { code: "args_not_allowed" }`:

- git takes a positive allowlist of read-only subcommands, exactly those `GitService.swift` runs: `status`, `diff`, `diff-tree`, `log`, `rev-parse`, `rev-list`, `merge-base`, `ls-files`, `for-each-ref`, and `symbolic-ref` in its read form only (`[--short] [-q] <name>`). Every other subcommand — `config`, `grep`, `worktree`, `init`, `bundle`, `format-patch`, `hash-object`, `cat-file`, aliases, … — is refused.
- git global options before the subcommand are limited to `--literal-pathspecs`, `--glob-pathspecs`, `--noglob-pathspecs`, `--icase-pathspecs`, `--no-pager`, `--no-optional-locks`, `--no-replace-objects` and `-c core.quotePath=<value>` (so `-C`, other `-c` keys, `--git-dir`, `--work-tree`, `--exec-path`, `--namespace`, `--config-env` are refused).
- Before `--`, `-o…` and any long option that abbreviates or extends `--git-dir`, `--work-tree`, `--exec-path`, `--namespace`, `--config-env`, `--upload-pack`, `--receive-pack`, `--output` (so also `--output-directory`), `--exec`, `--ext-diff`, `--textconv` is refused.
- `--no-index` is allowed only as `diff [options] --no-index [options] -- <a> <b>` where each of `<a>`, `<b>` is `/dev/null` or a relative path of plain names.
- `gh` runs only `pr list|diff|view|status|checks`, `auth status` and `repo view` (no extensions or aliases).

### CLI

- `pu --machine <name> <command…>` — conflicts with `--socket`. Uses the app's live forward when its socket answers; otherwise opens a private `ssh -N -L` forward on `cli-<pid>.sock` for the invocation, records the ssh pid in `cli-<pid>.sock.pid`, and closes both on exit. SIGTERM, SIGHUP and SIGINT also close it before the CLI exits. Before opening one it sweeps files whose CLI pid is dead, sending SIGTERM to a recorded pid only if it is still an `ssh` process whose arguments contain `-L <machine dir>/cli-<pid>.sock:`. Never starts a daemon. Project-scoped commands require `--project <remote root>`; agent-targeted commands resolve by id on that machine.
- `pu machine list|add <name> <ssh_host>|remove <name>|status [<name>]|start <name>`.

## Edge Cases

- **Two forwards for one machine** (app and CLI): the CLI reuses a live app socket and otherwise uses a per-invocation socket, so they never unlink each other's socket.
- **Remote home with spaces**: the remote socket path is passed as a single argv element.
- **Daemon restarted remotely**: the forward stays up; requests reconnect per call and streams resubscribe like local ones.
- **Large files**: reads over `max_bytes` return `truncated: true`; the editor treats them like local files over the preview limit. Writes are bounded by the 1 MB request line, so saves above ~750 KB of content are refused with a clear error.
- **Old remote daemon (protocol 8)**: shown as incompatible; the user upgrades PurePoint on that machine.
- **Agent/worktree id collision across machines**: ids are 8 random base36 characters (~2.8×10¹² space), so the app's terminal cache and pane surfaces key agents by id alone. A collision would only arise from copying a daemon's `.pu` state between machines; the CLI is unaffected because `--machine` scopes every request to one daemon.
- **Two app instances, one machine**: the instance holding `~/.pu/machines/<id>/forward.lock` (flock) owns the forward; another instance follows that forward and never launches or ends one of its own.
- **Remote layout changed elsewhere** (another Mac, or `pu grid` on the machine): layout saves are conditional on the document's modification time; on a conflict the app adopts the machine's document instead of overwriting it, and quitting only writes layouts with unsaved changes.
- **Remote files the editor cannot save** (over the ~750 KB request limit) open read-only; files over the 1 MB read limit show why they cannot open. Local copies for other apps are limited to 8 MB and removed when the app quits.
- **Slow or stalled links**: every remote file and git call runs off Swift's shared concurrency pool (a dedicated queue per git service and for host I/O), so a degraded machine cannot stall local projects.
