# Getting Started

This guide walks you from install to your first agent.

## Prerequisites

- **macOS** (PurePoint is macOS-only for now)
- **git** with worktree support
- At least one AI coding CLI: [Claude Code](https://docs.anthropic.com/en/docs/claude-code), [Codex](https://github.com/openai/codex), or [OpenCode](https://github.com/opencode-ai/opencode)

## Install

Download the latest `.dmg` from [Releases](https://github.com/2witstudios/purepoint/releases).

Open the DMG, drag PurePoint to Applications, and launch. The app installs the `pu` CLI to `~/.pu/bin/pu` on first launch.

Add to your shell config (`~/.zshrc` or `~/.bashrc`):

```sh
export PATH="$HOME/.pu/bin:$PATH"
```

Reload your shell, then verify:

```sh
pu health
```

## Initialize a workspace

Navigate to any git project and initialize:

```sh
cd ~/my-project
pu init
```

This creates `.pu/` with a manifest and default config. See [Directory Structure](../reference/directory-structure.md) for details.

## Spawn your first agent

```sh
pu spawn "fix the typo in README" --name fix-typo
```

This:
1. Creates a git worktree at `.pu/worktrees/wt-{id}` on branch `pu/fix-typo`
2. Spawns a Claude agent in that worktree
3. Sends the prompt to the agent

Check status:

```sh
pu status
```

Watch the agent's output:

```sh
pu logs ag-{id}
```

Or attach to the agent's terminal interactively:

```sh
pu attach ag-{id}
```

Press `Ctrl+C` to detach without killing the agent.

## Use the macOS app

Open PurePoint from Applications. Add your project directory. You'll see:

- **Sidebar**: All worktrees and agents with live status indicators
- **Terminal**: Click any agent to view its terminal
- **Command palette** (`Cmd+N`): Spawn new agents or swarms

## Spawn with different agent types

```sh
pu spawn "refactor tests" --agent codex --name refactor     # Codex
pu spawn "review the PR" --agent opencode --name review     # OpenCode
pu spawn --root --agent terminal                             # Plain terminal
```

## Spawn from a template

```sh
pu spawn --template code-review --var BRANCH=main --name review
```

See [Templates and Definitions](workflows/templates-and-definitions.md) for creating templates.

## Review agent work

View diffs across all worktrees:

```sh
pu diff
pu diff --worktree wt-abc12345    # specific worktree
pu diff --stat                     # file summary only
```

## Clean up

Kill a specific agent:

```sh
pu kill --agent ag-xyz98765
```

Remove a worktree, its agents, and branches:

```sh
pu clean --worktree wt-abc12345
```

Clean everything:

```sh
pu clean --all
```

## Next steps

- [CLI Reference](cli-reference.md) -- all commands and flags
- [Configuration](configuration.md) -- customize agent behavior
- [Templates and Definitions](workflows/templates-and-definitions.md) -- reusable prompts and agent types
- [Scheduling and Triggers](workflows/scheduling-and-triggers.md) -- automate agent workflows
- [Concepts](concepts.md) -- the mental model
