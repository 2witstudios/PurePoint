# PurePoint

An agent-first coding workspace.

[![CI](https://github.com/2witstudios/purepoint/actions/workflows/rust.yml/badge.svg)](https://github.com/2witstudios/purepoint/actions/workflows/rust.yml)
[![macOS](https://github.com/2witstudios/purepoint/actions/workflows/macos.yml/badge.svg)](https://github.com/2witstudios/purepoint/actions/workflows/macos.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
![Platform: macOS](https://img.shields.io/badge/platform-macOS-lightgrey)

![PurePoint — 10 agents streaming across 10 worktrees](docs/images/single-terminal-with-many-worktrees-in-sidebar.png)

## The Idea

IDEs were built for humans writing code. PurePoint is built for a world where agents write code and humans direct the work.

In basketball, a pure point guard doesn't score — they read the court, call the play, and put the ball in the right hands. That's the model. You are the point guard. Agents are your players. PurePoint is the court.

Unlike headless agent swarms where you prompt-and-forget, PurePoint gives you workspaces you **coach**. You see every terminal. You step in, redirect, course-correct. The UI is designed like productivity software — think of agents as collaborators, not background jobs.

## How It Works

PurePoint has two mental models:

**Workstations** are persistent. You spawn agents, direct their work, and work alongside them. Three agents reviewing the same PR from different angles — one command, three panes, three perspectives on one worktree.

**Exploratory swarms** are transient. Spawn 20 agents with the same greenfield prompt, let them each take a different approach, then compare results. You use the information, not necessarily the code. Ten agents each fixing a different issue — one swarm, ten worktrees, ten branches, all in parallel.

![Swarm definition with roster and execution config](docs/images/swarm-screen.png)

## Features

- **Pane grid** — split your workspace into any arrangement of terminal panes, each showing a live agent
- **Diff viewer** — review unstaged changes and PR diffs across worktrees without leaving the app
- **Swarms** — named plays defining which agents run, what prompts they get, and how worktrees are allocated
- **Prompt templates** — reusable prompts with variable substitution that resolve at spawn time
- **Agent definitions** — save custom agent configurations with specific types, prompts, and tags
- **Command palette** — keyboard-driven control over your entire workspace
- **Customizable hotkeys** — rebind every action; navigate without a mouse
- **Point Guard chat** — conversational interface for directing work across your workspace
- **Auto-resume** — agents and layout persist across app restarts
- **Schedules** — swarms on cron; a nightly security review or weekly dependency audit, results waiting when you open the app

| ![Agent definitions](docs/images/agents-screen.png) | ![Diff viewer](docs/images/diff-viewer.png) |
|---|---|
| ![Point Guard chat](docs/images/past-convo-view.png) | ![Customizable hotkeys](docs/images/hotkeys-menu.png) |
| ![Weekly schedule](docs/images/weekly-cal-schedule.png) | ![Point Guard](docs/images/point-guard-agent.png) |

## Getting Started

Releases coming soon. For now, build from source:

<details>
<summary>Build from source</summary>

Prerequisites: macOS, Rust 1.88+, Xcode, [just](https://github.com/casey/just)

```sh
git clone https://github.com/2witstudios/purepoint.git
cd purepoint
just setup
just build-app
```

</details>

The app installs the `pu` CLI to `~/.pu/bin/pu` on launch. Add it to your PATH:

```sh
export PATH="$HOME/.pu/bin:$PATH"
```

Then in any git project:

```sh
pu init
pu spawn "fix the typo in README"
```

## The `pu` CLI

`pu` is the command-line interface to PurePoint. All commands support `--json` for structured output.

| Command | Description |
|---|---|
| `pu init` | Initialize a PurePoint workspace |
| `pu spawn <prompt>` | Spawn an agent in a new worktree |
| `pu status` | Show workspace status |
| `pu kill` | Kill agents (by agent, worktree, or all) |
| `pu attach <agent>` | Attach to an agent's terminal |
| `pu logs <agent>` | View agent output logs |
| `pu health` | Check daemon health |
| `pu send <agent> <text>` | Send text or keys to an agent's terminal |
| `pu prompt list\|show\|create\|delete` | Manage saved prompt templates |
| `pu agent list\|show\|create\|delete` | Manage saved agent definitions |
| `pu swarm list\|show\|create\|delete\|run` | Manage and run swarm compositions |
| `pu grid show\|split\|close\|focus\|assign` | Control the pane grid layout |
| `pu schedule list\|show\|create\|delete\|enable\|disable` | Manage scheduled tasks |

Run `pu --help` for full usage.

## Current Status

macOS only. Linux TUI is planned.

PurePoint is early and under active development — the core works, but some features are still in design. See [`docs/`](docs/) for specs and architecture.

## Building from Source

<details>
<summary>Development commands</summary>

All tasks use [just](https://github.com/casey/just). Rust 1.88 is pinned via `rust-toolchain.toml`.

```sh
just fmt          # Format Rust code
just lint         # Run clippy lints
just test         # Run all Rust tests
just build-app    # Build the macOS app
just test-app     # Run macOS tests
just ci           # Run everything (fmt-check + lint + test + deny + build-app + test-app)
```

</details>

## License

MIT — [2wit Studios](https://github.com/2witstudios)
