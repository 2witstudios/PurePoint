# Concepts

PurePoint orchestrates AI coding agents in isolated git worktrees. This page defines the vocabulary used throughout the documentation.

## Worktree

A git worktree is a separate checkout of your repository with its own working directory and branch. PurePoint creates worktrees automatically when you spawn agents, so each agent works in isolation without interfering with your main branch or other agents.

Worktrees live in `.pu/worktrees/` and use branches prefixed with `pu/` (e.g., `pu/fix-auth`).

## Agent

An agent is a process running in a PTY (pseudo-terminal) inside a worktree. Agents are typically AI coding tools like Claude Code, Codex, or OpenCode, but can also be plain terminal sessions.

### Agent types

| Type | Command | Default behavior | Description |
|---|---|---|---|
| `claude` | `claude` | `--dangerously-skip-permissions` | Claude Code CLI |
| `codex` | `codex` | `--full-auto` | OpenAI Codex CLI |
| `opencode` | `opencode` | (none) | OpenCode CLI |
| `terminal` | `$SHELL` | (none) | Plain terminal, no AI |

### Agent status

Agents have exactly three states:

| Status | Meaning |
|---|---|
| `streaming` | Active, producing output |
| `waiting` | Alive but idle (shell prompt detected or idle > 30s), or suspended |
| `broken` | Process exited (terminal state) |

### Root agents

Most agents live inside a worktree. A **root agent** runs in the project root without creating a worktree. Use root agents for read-only tasks, research, or orchestration.

### Point Guard

The Point Guard screen is a root terminal for interactive work. It auto-launches your configured coding agent and shows conversation history in the sidebar. This is where you direct work, delegate tasks, and coordinate agents.

## Agent def

An agent def (agent definition) is a saved, reusable agent configuration stored as a YAML file. It specifies:

- **Agent type** (claude, codex, opencode, terminal)
- **Template** or **inline prompt** to use
- **Tags** for categorization
- **Icon** for the command palette

Agent defs live in `.pu/agents/` (local) or `~/.pu/agents/` (global). Local defs shadow global defs with the same name.

## Template

A template is a reusable prompt with variable substitution. Templates are markdown files with optional YAML frontmatter, stored in `.pu/templates/` (local) or `~/.pu/templates/` (global).

Variables use `{{VAR}}` syntax and are substituted at spawn time via `--var KEY=VALUE`.

```markdown
---
name: code-review
description: Review code for quality
agent: claude
---
Review the code on branch {{BRANCH}} for {{SCOPE}}.
```

## Swarm

A swarm is a multi-agent composition. It defines a **roster** of agent defs, the number of **worktrees** to create, and optionally includes a terminal per worktree. Swarms let you run coordinated multi-agent workflows with a single command.

Swarm defs live in `.pu/swarms/` (local) or `~/.pu/swarms/` (global).

## Schedule

A schedule triggers agent or swarm spawns on a recurring cadence. Recurrence options: `none` (one-shot), `hourly`, `daily`, `weekdays`, `weekly`, `monthly`. Schedules reference an agent def, swarm def, or inline prompt as their trigger.

Schedule defs live in `.pu/schedules/` (local) or `~/.pu/schedules/` (global).

## Trigger

A trigger defines event-driven automation. When a trigger event fires, a sequence of actions executes:

| Event | When it fires |
|---|---|
| `agent_idle` | Agent finishes work and awaits input |
| `pre_commit` | Git pre-commit hook runs |
| `pre_push` | Git pre-push hook runs |

Actions can **inject** text into an agent's terminal or run **gate** commands (shell commands that must pass before proceeding). Gates support retries.

Trigger defs live in `.pu/triggers/` (local) or `~/.pu/triggers/` (global).

## Scope

All definitions (templates, agent defs, swarm defs, schedules, triggers) follow the same scoping pattern:

| Scope | Location | Applies to |
|---|---|---|
| Local | `.pu/{type}/` in the project | This project only |
| Global | `~/.pu/{type}/` in your home | All projects |

Local definitions take priority over global definitions with the same name.

## Daemon

The PurePoint daemon (`pu-engine`) is a background process that manages agent lifecycles, worktrees, schedules, and triggers. The CLI auto-starts the daemon on first use. The macOS app launches the daemon in managed mode.

Communication between CLI/app and daemon uses NDJSON over a Unix socket at `~/.pu/daemon.sock`.

## Manifest

The manifest (`.pu/manifest.json`) is the single source of truth for workspace state. It tracks all worktrees, agents, their statuses, and metadata. Both the daemon and the macOS app read and write it using atomic operations and file locking.

## Pane grid

The pane grid is the macOS app's terminal layout system. Split your workspace into any arrangement of terminal panes (vertical, horizontal, nested). Each pane displays one agent's terminal. Layouts persist across sessions.
