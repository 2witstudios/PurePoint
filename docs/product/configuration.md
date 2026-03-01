# Configuration

**Maturity: SEED** | ID Prefix: CFG | Dependencies: none

## Purpose

User and project configuration: settings, defaults, agent templates, environment detection, and config file management.

## Open Questions

? [CFG-001] Should config be stored in YAML files, in the SQLite database, or both (files for user editing, DB for runtime)?

? [CFG-002] How should config changes be applied — require daemon restart, hot-reload, or per-command resolution?

## Conceptual Model

```
Config hierarchy:
  Built-in defaults
    Global config (~/.pu/config.yaml or daemon.db)
      Project config (.pu/config.yaml or pu.db)
        Command-line flags (highest priority)

Config domains:
  daemon: socket path, log level, worker threads
  agent: default type, default prompt, timeout
  tmux: session prefix, pane layout
  git: worktree location, branch prefix
  ui: refresh interval, terminal font, theme
```
