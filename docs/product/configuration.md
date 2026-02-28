# Configuration

**Maturity: SEED** | ID Prefix: CFG

## Purpose

User and project configuration: settings, defaults, agent templates, environment detection, and config file management.

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
