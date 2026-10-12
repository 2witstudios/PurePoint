# Changelog

All notable changes to PurePoint are documented here. Format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

### Fixed
- Agent Rust build caches no longer fill the disk. Agents in a worktree now build into `<worktree>/target` (the daemon sets `CARGO_TARGET_DIR`), which is deleted with the worktree, and the daemon periodically removes abandoned `/tmp/purepoint-*-target` caches, sweeping harder when the disk is low.
- `pu spawn` and `pu send` to Claude agents now deliver the full text as one submitted turn, or fail with a non-zero exit. Typing starts only once the agent's input box is on screen, Enter is pressed only after the box shows the complete text (retyping if characters were dropped), and success requires the box to clear and the turn to be echoed. Trigger injection uses the same path. Callers no longer need an empty `pu send` "nudge"; remove it, since it can submit a fragment or a duplicate.

### Changed
- YAML files under `.pu/` and `~/.pu/` are now read and written with the safe-Rust `serde-saphyr` library instead of the unmaintained `unsafe-libyaml` stack (RUSTSEC-2024-0449). Existing files load with the same values. Newly written files use double quotes and also quote YAML 1.1 look-alikes such as `"yes"` and `"on"`. A few hand-written edge cases now load where they used to fail: a repeated key keeps its last value, `1_000` reads as a number, and a `~` document reads as empty. A plain `~`/`null` in a text field reads as empty text rather than the literal word. An unquoted leading-zero number such as `012` in a schedule `trigger:` must now be quoted.

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
