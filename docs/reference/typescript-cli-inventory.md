# TypeScript CLI Inventory

Source map of the existing TypeScript CLI implementation (`src/`).

## Commands (`src/commands/`)

| File | Purpose | Size |
|---|---|---|
| spawn.ts | Spawn worktree + agents (most complex command) | 16K |
| cron.ts | Cron scheduler daemon (start/stop/list/add/remove) | 10K |
| init.ts | Initialize Point Guard in a git repo | 7.1K |
| reset.ts | Kill all agents, remove all worktrees, wipe manifest | 6.8K |
| status.ts | Show status of worktrees and agents | 5.5K |
| clean.ts | Remove worktrees in terminal states | 4.8K |
| list.ts | List available templates, swarms, prompts | 4.2K |
| aggregate.ts | Collect results from agents | 3.5K |
| pr.ts | Create GitHub PR from worktree branch | 3.2K |
| wait.ts | Wait for agents to reach terminal state | 3.1K |
| recover.ts | Recover agents after tmux crash | 2.9K |
| restart.ts | Restart an agent in same worktree | 2.9K |
| worktree.ts | Create standalone worktree | 2.7K |
| send.ts | Send text to agent's tmux pane | 2.3K |
| logs.ts | View agent pane output | 2.2K |
| ui.ts | Open native dashboard | 2.2K |
| diff.ts | Show changes in worktree branch | 1.8K |
| attach.ts | Attach to worktree/agent tmux pane | 1.8K |
| prompt.ts | Spawn using named prompt | 1.2K |
| install-dashboard.ts | Download and install macOS dashboard | 3.6K |

## Core (`src/core/`)

| File | Purpose | Size |
|---|---|---|
| tmux.ts | tmux operations (session/window/pane management) | 8-11K |
| agent.ts | Agent lifecycle (spawn, status detection, kill) | 8.4K |
| recover.ts | Recovery logic (re-scan tmux after crash) | 9-10K |
| cron.ts | Cron scheduler logic | 6.7K |
| swarm.ts | Swarm template execution | 4.4K |
| cleanup.ts | Worktree cleanup operations | 4.0K |
| manifest.ts | Manifest read/write with locking | 3.8K |
| schedule.ts | Schedule management | 3.5K |
| self.ts | Self-update logic | 2.7K |
| worktree.ts | Git worktree operations | 2.4K |
| template.ts | Template loading and validation | 2.4K |
| config.ts | Configuration management | 2.1K |
| prompt.ts | Prompt file loading | 1.5K |
| env.ts | Environment detection | 1.3K |
| terminal.ts | Terminal utilities | 1.3K |
| pr.ts | Pull request utilities | 754B |

## Types (`src/types/`)

| File | Purpose |
|---|---|
| manifest.ts | Core data model: Manifest, WorktreeEntry, AgentEntry, status enums |

## Lib (`src/lib/`)

| File | Purpose | Size |
|---|---|---|
| errors.ts | Error types (PpgError) | 2.3K |
| output.ts | Output formatting (tables, colors) | 3.0K |
| paths.ts | Path utilities | 2.6K (tests) |
| name.ts | Name generation | 1.1K |
| id.ts | ID generation | 1.4K (tests) |

## Bundled (`src/bundled/`)

| File | Purpose |
|---|---|
| conductor-context.ts | Default conductor context | 
| prompts.ts | Bundled prompts |
| swarms.ts | Bundled swarm templates |
| templates.ts | Bundled agent templates |

## Entry Point

| File | Purpose | Size |
|---|---|---|
| cli.ts | Main CLI definition (27 commands via Commander.js) | 15K |
