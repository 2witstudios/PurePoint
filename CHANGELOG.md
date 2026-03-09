# Changelog

All notable changes to PurePoint are documented here. Format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

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
