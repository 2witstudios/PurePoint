# Configuration

**Maturity: DECIDED** | ID Prefix: CFG | Dependencies: none

## Purpose

User and project configuration: settings, defaults, agent templates, environment detection, and config file management.

## Conceptual Model

```
Config hierarchy (highest priority wins):
  Built-in defaults (agents launch bare: no launch args)
    Global config (~/.pu/, incl. machine launch args in agent-settings.yaml)
      Project config (.pu/)
        Command-line flags

Config domains:
  daemon: connection, logging, performance
  agent: default type, default prompt, timeout
  worktree: location, branch prefix
  ui: refresh, appearance
```

## Decisions

! [CFG-001] Store machine settings (launch args for all projects, agent status tracking) in `~/.pu/agent-settings.yaml`, separate from project configuration — they cover CLI and future projects and survive repository merges. There is no Codex-specific permission policy: Codex full access is a launch arg the user writes (`--dangerously-bypass-approvals-and-sandbox`), like any other flag (CFG-004). The former `codexYolo` key is ignored when loading and dropped on the next save.

! [CFG-002] Read the machine settings on each config resolution — future spawn and resume operations apply changes without a daemon restart or project sync. Existing agent processes must restart to pick them up. Save the file under a lock with atomic replacement; malformed settings return an error rather than being silently replaced.

! [CFG-003] Launch agents safely by default. With no configured launch args, every agent type launches its bare base command (`claude`, `codex`, `opencode`, `$SHELL`) with no permission, sandbox or auto-approve flags. Worktrees isolate git state, not the OS, so bypassing approvals must be an explicit user choice. PurePoint never adds tool-specific flags in spawn, resume, schedule or swarm launch paths.

! [CFG-004] Launch args are a free-form string the user owns. PurePoint knows only each agent's base command and appends the user's string, split shell-style, without interpreting flags — so CLI flag changes in Claude Code, Codex or OpenCode need no PurePoint change. Settings shows a base command and one text field per agent, with no per-flag controls.

! [CFG-005] Machine-level launch args in `~/.pu/agent-settings.yaml` let the owner set a personal default (e.g. auto mode) for every project without making it the product default. Precedence: `pu spawn --no-auto` (bare) > project `launchArgs` when set > machine `launchArgs.<agent>` > none. They are read on each config resolution like CFG-002.

## Requirements and Interfaces

- Given no project or machine launch args, Claude, Codex, OpenCode and the
  terminal should spawn and resume with their bare command and no extra flags.
- Given `launchArgs: "--model opus --append-system-prompt 'be terse'"`, the
  agent should launch with the four arguments `--model`, `opus`,
  `--append-system-prompt`, `be terse` appended to its command.
- Given legacy YAML `launchArgs: [..]` or `launchArgs: []`, the config should
  load with the same arguments as before; PurePoint writes the string form back.
- Given an unbalanced quote in a project or machine launch args string, config
  resolution should fail with an error and never fall back to permission flags.
- Given machine `launchArgs.claude` and no project `launchArgs`, Claude should
  launch with the machine args in every project, including new ones.
- Given a project `launchArgs` (including `""`), it should take precedence over
  the machine args for that agent.
- Given `pu spawn --no-auto`, the agent should launch its bare command,
  ignoring project and machine launch args; `--agent-args` still apply.
- Given a Codex `--full-auto` or `--approval-mode=full-auto` the user wrote, it
  should be rewritten to `--sandbox workspace-write --ask-for-approval
  on-request`; no flags the user did not write are added.
- YAML: `launchArgs: {<agent type>: string}` in `~/.pu/agent-settings.yaml`,
  default empty. IPC: `update_global_agent_settings` takes optional
  `launch_args` (agent type → string; blank removes), and
  `global_agent_settings_report` returns `launch_args`. `config_report` agents
  add `project_launch_args`, `machine_launch_args` and `resolved_command`.

- Given a machine settings file that still has `codexYolo` from an older
  version, it should load without error, the key should have no effect, and
  the next save should drop it.
- Given a save fails, Settings should show the error and retain the last
  confirmed state. Settings should be available without an open project.
- Daemon IPC: `get_global_agent_settings`, `update_global_agent_settings`
  (optional `agent_status_tracking: bool`; an omitted field keeps its stored
  value; a `codex_yolo` field from older clients is ignored), response
  `global_agent_settings_report` (`agent_status_tracking`, `launch_args`).
- YAML: `agentStatusTracking: bool`, default `true`. When `false`, new and
  resumed agents launch without status adapters, so they report no status.
  Tracking works with any version of Claude Code, Codex, OpenCode or Pi;
  adapters record the observed version but never gate on it.

## Research Notes

### [CFG-001] Storage and cross-project agent permissions
**Researched: 2026-10-09** | **Revised: 2026-10-21** — the Codex YOLO policy
below was removed once machine-level launch args (CFG-005) existed; a user who
wants Codex full access everywhere writes the flag in `launchArgs.codex`.

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

Recommendation: resolve the machine settings on each config read. Existing
Codex processes keep their startup permissions and must be restarted.
