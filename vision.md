# purepoint

An agent-first coding workspace.

## The idea

IDEs were built for humans writing code. Purepoint is built for a world
where agents write code and humans direct the work.

In basketball, a pure point guard doesn't score — they read the court, call
the play, and put the ball in the right hands. That's the model. You are the
point guard. Agents are your players. Purepoint is the court.

## How it works

You describe what needs to happen. Purepoint sets up isolated workstations
— git worktrees with their own branches, terminals with live agents,
prompts tailored to the task. You see everything. You can step into any
terminal and work alongside any agent. These aren't background jobs. These
are workstations you work out of.

Three agents reviewing the same PR from different angles — one command,
three panes, three perspectives on one worktree. Ten agents each fixing a
different issue — one swarm, ten worktrees, ten branches, all in parallel.

## The building blocks

**Swarms** — named plays. Which agents run, what prompts they get, whether
they share a worktree or each get their own. Save them, reuse them,
schedule them.

**Prompt templates** — reusable prompts with variables that resolve at
spawn time. The right context for the right task, every time.

**Schedules** — swarms on cron. A nightly security review. A weekly
dependency audit. Results waiting when you open the app.

## The product

A Rust engine that manages worktrees, agents, and orchestration. Lightweight
and trusted. On macOS, a beautiful native Swift app — sidebar, pane grid,
command palette, config editors. On Linux, a terminal UI. On your phone, a
remote connection. The engine is the constant. The interface is native to
each platform.

## The surface

`pu` is the command. `.pu/` is the config. `pu/<name>` are the branches.

purepoint.dev
