# Configuration

**Maturity: DECIDED** | ID Prefix: CFG | Dependencies: none

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

## Decisions

! [CFG-001] Store the opt-in Codex YOLO policy in `~/.pu/agent-settings.yaml`, separate from project configuration — it covers CLI and future projects and survives repository merges. When enabled, it overrides Codex permission flags in configured commands and launch args while preserving wrapper prefixes and unrelated options. Other agent types are unaffected. When disabled, project settings apply unchanged.

! [CFG-002] Read the machine policy on each config resolution — future spawn and resume operations apply changes without a daemon restart or project sync. Existing agent processes must restart to change permissions. Save the file under a lock with atomic replacement; malformed settings return an error rather than silently enabling YOLO.

## Requirements and Interfaces

- Given YOLO for all projects is enabled, Codex should launch and resume with
  `--dangerously-bypass-approvals-and-sandbox` and `--no-daemon` in every project.
- Given a command wraps Codex, permission normalization should preserve the
  wrapper executable and its arguments, model choices, web search and other
  non-permission options.
- Given the policy is disabled or missing, project configuration should retain
  its existing behavior without being rewritten.
- Given a save fails, Settings should show the error and retain the last
  confirmed state. Settings should be available without an open project.
- Daemon IPC: `get_global_agent_settings`, `update_global_agent_settings`
  (`codex_yolo: bool`), response `global_agent_settings_report`.
- YAML: `codexYolo: bool`, default `false`. The explicit machine policy takes
  precedence over project Codex permission flags; other configuration follows
  the normal hierarchy.

## Research Notes

### [CFG-001] Storage and cross-project agent permissions
**Researched: 2026-10-09**

`pu-core/src/config.rs` already reads project YAML; agent Settings writes it via
daemon IPC. Codex's built-in launch flags override the user's Codex defaults,
and repository merges or GitHub Desktop stashes can restore old project flags.

- Copying a YOLO preset into every project uses the existing update API, but
  needs a sync action for new projects and can be undone by repository changes.
- App UserDefaults avoids repository changes, but CLI, schedules and daemon
  resume cannot read the preference through the existing config loader.
- Machine-level YAML in `~/.pu/` is readable by all launch paths and survives
  project changes. An explicit cross-project YOLO policy can override permission
  flags at config resolution without rewriting project files.

Recommendation: machine-level YAML for the opt-in Codex YOLO policy; retain
project YAML for agent commands and other launch options.

### [CFG-002] Applying changes
All spawn, resume and config-report paths call `load_config_strict`; per-command
resolution makes changes available without restarting the PurePoint daemon.
A startup snapshot would require a daemon restart, and copying changes into
projects would require synchronization and partial-failure reporting.

Recommendation: resolve the machine policy on each config read. Existing
Codex processes keep their startup permissions and must be restarted.
