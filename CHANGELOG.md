# Changelog

All notable changes to PurePoint are documented here. Format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

### Added
- The daemon and CLI now run on Linux servers. Releases include static `pu` and `pu-engine` builds for x86_64 and aarch64 Linux, and `scripts/install.sh` installs them into `~/.pu/bin`. See `docs/guide/linux-server.md`.
- `pu daemon start|stop|status|install|uninstall`. `install` runs the daemon as a user service (systemd on Linux, launchd on macOS) that restarts after a crash and, on Linux with lingering enabled, keeps running with no one logged in.
- **Machines over SSH** in the macOS app. Add a machine (a name plus an `~/.ssh/config` alias or `user@host[:port]`) from the sidebar footer, and its PurePoint daemon's projects appear under a new MACHINES section. Remote projects work like local ones: open terminals, spawn agents, create worktrees, browse, edit and save files, and review git changes, all running on that machine. The app connects with your own `ssh` configuration, never starts a remote daemon on its own (use Start Daemon), and leaves remote agents running when you disconnect or quit. Machines are shared with `pu machine` through `~/.pu/machines.json`.

### Fixed
- A daemon started by the CLI no longer dies when the terminal or SSH session that started it closes. It now runs in its own session and ignores SIGHUP.
- `pu` finds `pu-engine` beside itself, in `~/.pu/bin` or in `~/.cargo/bin` when it is not on `PATH`. A non-interactive SSH session's minimal `PATH` no longer stops it from starting the daemon.
- On Linux, the daemon reads `/proc` instead of running `ps` and `lsof`, so process cleanup and Codex conversation lookup work on minimal servers. Shells fall back to the account's login shell, then `/bin/sh`, instead of `/bin/zsh`.
- Agent Rust build caches no longer fill the disk. Agents in a worktree now build into `<worktree>/target` (the daemon sets `CARGO_TARGET_DIR`), which is deleted with the worktree, and the daemon periodically removes abandoned `/tmp/purepoint-*-target` caches, sweeping harder when the disk is low.
- `pu spawn` and `pu send` to Claude agents now deliver the full text as one submitted turn, or fail with a non-zero exit. Typing starts only once the agent's input box is on screen, Enter is pressed only after the box shows the complete text (retyping if characters were dropped), and success requires the box to clear and the turn to be echoed. Trigger injection uses the same path. Callers no longer need an empty `pu send` "nudge"; remove it, since it can submit a fragment or a duplicate.

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
