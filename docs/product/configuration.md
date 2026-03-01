# Configuration

**Maturity: SEED** | ID Prefix: CFG | Dependencies: none

## Purpose

User and project configuration: settings, defaults, agent templates, environment detection, and config file management.

## Conceptual Model

```
Config hierarchy (highest priority wins):
  Built-in defaults
    Global config (~/.pu/)
      Project config (.pu/)
        Command-line flags

Config domains:
  daemon: connection, logging, performance
  agent: default type, default prompt, timeout
  worktree: location, branch prefix
  ui: refresh, appearance
```

## Open Questions

? [CFG-001] Should config be stored in files, in a database, or both (files for user editing, DB for runtime)?

? [CFG-002] How should config changes be applied — require daemon restart, hot-reload, or per-command resolution?
