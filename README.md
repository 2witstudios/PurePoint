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

## Build Your Team

Define reusable agent types — a PR reviewer, a security auditor, a refactorer. Each gets a name, a system prompt, and tags. Build your roster once, deploy it everywhere.

![Agent definitions in the Agents Hub](docs/images/agents-screen.png)

## Call the Play

Assemble agents into swarms — named plays. A `pr-review` swarm with 3 reviewers on 1 worktree, or a `simplify` swarm with 7 agents across 7 worktrees. Define the roster, set the execution config, and hit Run.

![Swarm definition with roster and execution config](docs/images/swarm-screen.png)

## Watch the Game

All agents stream live in the sidebar. Click any worktree to watch its terminal. Every agent visible, every branch isolated.

PurePoint has two mental models. **Workstations** are persistent — you spawn agents, direct their work, and work alongside them. Three agents reviewing the same PR from different angles, three panes, three perspectives. **Exploratory swarms** are transient — spawn 20 agents with the same greenfield prompt, let them each take a different approach, then compare results. You use the information, not necessarily the code.

![Watch the Game — worktree terminal streaming live](docs/images/single-terminal-with-many-worktrees-in-sidebar.png)

## Coach the Players

Point Guard is your conversational interface. Ask it to spawn agents, check status, redirect work — without leaving the chat.

![Point Guard — fresh conversation](docs/images/point-guard-agent.png)

Ask a question and Point Guard calls tools on your behalf, orchestrating agents and reporting back with results.

![Point Guard conversation with tool calls](docs/images/past-convo-view.png)

## Review the Results

When agents finish, review their work without leaving PurePoint. The diff viewer shows unstaged changes and PR diffs per worktree — modified files highlighted, changes inline.

![Diff viewer with PR changes](docs/images/diff-viewer.png)

## Automate the Routine

Schedule swarms to run on a cadence. A nightly security review, a weekly dependency audit — results waiting when you open the app.

![Weekly schedule calendar](docs/images/weekly-cal-schedule.png)

## Make It Yours

Rebind every action. Navigate without a mouse. The settings panel lets you customize hotkeys for all workspace actions and navigation.

![Customizable hotkeys settings](docs/images/hotkeys-menu.png)

## And More

- **Pane grid** — split your workspace into any arrangement of terminal panes
- **Prompt templates** — reusable prompts with variable substitution, resolved at spawn time
- **Auto-resume** — agents and layout persist across app restarts

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
