# Changelog

All notable changes to PurePoint are documented here. Format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

### Changed
- Release builds now compile in jobs that hold no secrets, and a separate job in a protected environment signs and notarizes the result. The Sparkle signing tool matches the Sparkle version the app ships (2.9.0) and is checked against a pinned SHA-256. Workflow actions are pinned to commit SHAs. The GitHub release is created once, with all of its assets.
- **Breaking: agents now launch without permission-skipping flags by default.** Claude Code used to start with `--dangerously-skip-permissions` and Codex with `--sandbox workspace-write --ask-for-approval on-request` whenever a project didn't set `launchArgs`. Now every agent launches with its bare command (`claude`, `codex`, `opencode`) and follows the tool's own approval settings. Worktrees isolate git state, not your filesystem or network, so skipping approvals is now something you opt into. To keep auto mode for yourself in every project, add it once to `~/.pu/agent-settings.yaml` (or Settings → Agents → Launch args for all projects):
  ```yaml
  launchArgs:
    claude: "--dangerously-skip-permissions"
  ```
  Projects that already set `launchArgs` are unaffected.
- `launchArgs` is now one free-form string, split like a shell command line (`launchArgs: "--model opus --append-system-prompt 'be terse'"`), so new or renamed CLI flags work without a PurePoint update. Existing list values (`launchArgs: [..]`, `launchArgs: []`) still load the same; PurePoint writes the string form when it saves. A project value, even `""`, takes precedence over the machine-level one.
- `pu spawn --no-auto` now launches the bare command and ignores project and machine-level launch args. `--agent-args` still apply. The choice is saved with the agent, so it still launches bare after `pu bench`/`pu play`, suspend/resume or a daemon restart.
- The daemon protocol is now version 11 because the launch args request and response shapes changed. Restart the daemon after updating (`pu` and the macOS app reject a v10 daemon with update guidance instead of appearing to save launch args that an old daemon drops).
- Settings → Agents replaces the per-flag pickers for Claude, Codex and OpenCode with one launch args field per agent next to its command, for this project and for all projects. Codex YOLO for all projects is unchanged.
- **Breaking:** `pu` no longer searches `PATH` or `~/.cargo/bin` when it auto-starts the daemon. It starts the `pu-engine` beside itself, then `~/.pu/bin/pu-engine`; a `pu` inside the macOS app starts only the app's own engine. To run another build, set `PU_ENGINE_PATH` to its path (or to a name to look up on `PATH`). The chosen binary must be owned by you or root (or, for the app's own engine, by whoever owns the app) and not writable by group or others. If none qualifies, the error says why each candidate was refused.
- The macOS app now installs `pu-engine` next to `pu` in `~/.pu/bin`, so `~/.pu/bin/pu` can start the daemon when the app isn't running. Both are copied to a temporary file and renamed into place, so a daemon running from `~/.pu/bin` is never modified underneath itself.
- The daemon socket is now always owner-only (0600) and the daemon accepts connections only from processes running as its own user. `~/.pu` is kept at mode 0700, and the daemon refuses to start if another user owns it.
- A request line over 1 MB now gets a `FRAME_TOO_LARGE` error and closes the connection, instead of a `PARSE_ERROR` that left the rest of the line to be read as the next request. This holds whatever bytes the line contains. Unreadable (non-UTF-8) input inside an attach, grid or status stream also closes the connection. A connection that sends no request within 30 seconds is closed.

### Fixed
- `$HOME`, `/` and ancestors of `$HOME` are never registered as projects, even when an older version left `~/.pu/manifest.json` behind. Such entries are removed from the project registry when the daemon starts. Requests that would create or change something under such a root (`init`, `spawn`, worktrees, templates, schedules and the rest) now fail with `project_root_not_allowed`. If an older version recorded agents under `$HOME`, you can still see, read and kill them there (`status`, `read`, `kill`). Move ongoing work to a real project directory.
- Request lines that arrive in several pieces are no longer cut short, whether an attach or grid stream is sending output meanwhile or the stream ends (for example when the agent exits) before the line is complete. A last request sent without a trailing newline before the client closes its side is still answered.

## [0.5.2] — 2026-10-15

### Fixed
- Point Guard no longer shows a blank agent screen when its root folder is too broad to open; it falls back to the chat surface instead.

## [0.5.1] — 2026-10-15

### Fixed
- The macOS sidebar's project and worktree rows keep their open/closed state across refreshes instead of snapping everything open again. The tree rebuilds on every data change (an unread count, an agent status, the 30 s machine inventory), and a rebuild forgot every row, so closing one row could reopen others. Closed rows now persist across launches, and a selection inside a closed section opens that section instead of going nowhere.
- Closing a row no longer closes the open rows inside it. AppKit announces a row's descendants when the row closes, and those were being recorded as closed, so reopening a project left its worktrees collapsed.
- A rebuild that lands before a project's rows have loaded no longer erases saved row state. Worktrees load after the project row on relaunch, and remote projects after their machine answers, so the sidebar briefly shows parents with nothing under them; nothing is pruned until every row holds the rows it will.
- Sidebar row identities escape their delimiters, so a project root that spells out another row's key cannot make two rows answer to one. This changes the persisted format, so rows closed by an earlier build come back open once.
- Machines added through `pu machine` while the macOS app is running now appear in the sidebar automatically; the app watches `~/.pu/machines.json`, reconnects safely, and handles removals and add races without a restart.
- Project roots that are too broad (home directory, `/`, etc.) are filtered out before the app opens them, so they no longer abort startup; the filtering persists across sessions.

## [0.5.0] — 2026-10-13

### Added
- The daemon and CLI now run on Linux servers. Releases include static `pu` and `pu-engine` builds for x86_64 and aarch64 Linux, and `scripts/install.sh` installs them into `~/.pu/bin`. See `docs/guide/linux-server.md`.
- `pu daemon start|stop|status|install|uninstall`. `install` runs the daemon as a user service (systemd on Linux, launchd on macOS) that restarts after a crash and, on Linux with lingering enabled, keeps running with no one logged in.
- **Machines over SSH** in the macOS app. Add a machine (a name plus an `~/.ssh/config` alias or `user@host[:port]`) from the sidebar footer, and its PurePoint daemon's projects appear under a new MACHINES section. Remote projects work like local ones: open terminals, spawn agents, create worktrees, browse, edit and save files, and review git changes, all running on that machine. The app connects with your own `ssh` configuration, never starts a remote daemon on its own (use Start Daemon), and leaves remote agents running when you disconnect or quit. Machines are shared with `pu machine` through `~/.pu/machines.json`.
- **Point Guard chat bubbles**. Point Guard replies as text-message-style bubbles with an animated typing indicator instead of a single block of text.

### Changed
- The Point Guard surface is darker, and Pi branding is gone.

### Fixed
- A daemon started by the CLI no longer dies when the terminal or SSH session that started it closes. It now runs in its own session and ignores SIGHUP.
- `pu` finds `pu-engine` beside itself, in `~/.pu/bin` or in `~/.cargo/bin` when it is not on `PATH`. A non-interactive SSH session's minimal `PATH` no longer stops it from starting the daemon.
- On Linux, the daemon reads `/proc` instead of running `ps` and `lsof`, so process cleanup and Codex conversation lookup work on minimal servers. Shells fall back to the account's login shell, then `/bin/sh`, instead of `/bin/zsh`.
- Agent Rust build caches no longer fill the disk. Agents in a worktree now build into `<worktree>/target` (the daemon sets `CARGO_TARGET_DIR`), which is deleted with the worktree, and the daemon periodically removes abandoned `/tmp/purepoint-*-target` caches, sweeping harder when the disk is low.
- `/pointguard` info reports no longer reach the model's context.
- Opening the deliberate-naming step no longer flashes the prompt layout first.
- A deferred search focus no longer steals the field focus of an initially selected item.

## [0.2.0] — 2026-03-09

### Added
- **Point Guard terminal** — replaced chat bubble UI with a full terminal that auto-launches Claude Code on startup, with IDE-style stream layout, conversation switching, and configurable launch command/permissions
- **Multi-agent conversations** — browse and resume conversations from Claude Code, Codex, and OpenCode in the sidebar
- **`pu watch`** — live terminal dashboard showing all agents and their status in real time
- **`pu bench` / `pu play`** — suspend and resume agents without killing them
- **`pu pulse`** — one-line workspace heartbeat: agent count, activity, idle status
- **Trigger system** — event-driven triggers that sequence agent workflows, with UI in Agents Hub
- **Agent configuration** — `--agent-args`, `launchArgs`, `--plan` mode, `--no-auto`, and Settings > Agents UI
- **Schedule event editing** — edit events after creation with color customization
- Comprehensive documentation: user guide, CLI reference, contributor guides, reference docs

### Fixed
- PTY file descriptor double-close and EINTR handling
- Terminal focus not restoring on pane click
- `pu send` race where Enter was swallowed by TUI paste mode
- Cmd+N stacking multiple command palettes and routing to wrong project
- Conversation resume using wrong project directory
- Retain cycle in CommandPalettePanel closures
- ManifestWatcher file descriptor double-close
- Unsafe env var mutation replaced with thread-local
- Replaced `ctrlc` crate with `tokio::signal` + terminal Drop guard
- Unicode-safe string truncation throughout
- Replaced panicking `.unwrap()` / `.expect()` with proper error propagation
