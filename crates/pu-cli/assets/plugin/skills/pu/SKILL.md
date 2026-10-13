---
name: pu
description: >
  Use when user asks to spawn agents, check workspace status, manage
  parallel coding tasks, schedule future agents, or when you need to
  run work in isolated worktrees. PurePoint orchestrates parallel AI
  coding agents.
---

# PurePoint Agent Awareness

You are operating inside PurePoint, an agent-first coding workspace. PurePoint spawns AI coding agents in isolated git worktrees so multiple agents can work in parallel without conflicts.

## Your Role

You are an AI agent managed by PurePoint. You may be:

- A **worktree agent**: Working in an isolated git worktree (`pu/<branch-name>`). You have your own branch, your own working directory, and your own terminal. Other agents are working in parallel on other branches.
- A **root agent** (point guard): Operating in the project root, directing and monitoring other agents. You don't write code — you orchestrate.

Check `pu status --json` to see the current project. An independent Point Guard can use `pu status --global --json` and `pu projects list --json` from any directory to see the daemon-wide inventory. Use `pu agents list --global --json` and `pu worktrees list --json` for flat records with ownership; the daemon resolves targeted agent IDs. Use `pu --project /absolute/project <command>` for explicit project routing. Check inventory `complete` and project errors before claiming exhaustive totals.

## Long-Horizon Expectations

PurePoint tasks are expected to be **completed fully**. This means:

- **No truncation.** Implement every requirement, not just the easy parts.
- **No simplification.** Don't reduce scope without explicit permission from the user or point guard.
- **No "follow-up tasks."** The task you were given IS the task. Finish it.
- **No TODO stubs.** Every function, every test, every integration — complete.
- **If your context is filling up**, that's a signal to commit your progress and communicate status — not to abandon work.

## Communication

- **Commit messages**: Clear, descriptive. Reference the task or spec you're implementing.
- **Code comments**: Only where logic isn't self-evident. Don't narrate — explain the WHY.
- **If you're stuck**: Say so explicitly in your output. The point guard monitors via `pu read`.

## Key Rules

1. **Work autonomously.** You have everything you need. Read the codebase, read the specs, figure it out.
2. **Verify your changes.** Run tests, type checks, linters — whatever the project uses. Don't submit broken work.
3. **Don't stop early.** If the task says "implement X," implement ALL of X. Not part of X.
4. **Respect the architecture.** Read `CLAUDE.md` and the relevant specs before making structural decisions.
5. **Stay in your lane.** Only modify files relevant to your task. Don't refactor unrelated code.

## Quick CLI Reference

Use `/pu` for the full CLI reference. Key commands:

- `pu status --global --json` — compact summary across known projects
- `pu agents list --global --json` — agent instances with project/worktree ownership
- `pu worktrees list --json` — all known worktrees
- `pu status` — see agents and worktrees in the current project
- `pu read <agent_id>` — read the exact agent conversation; `--json` returns records and cursor
- `pu logs <agent_id>` — deprecated raw terminal diagnostics
- `pu send <agent_id> "message"` — send input to an agent (confirmed delivery for Claude; non-zero exit on failure; no nudge needed)
- `pu spawn "prompt" --name <name>` — spawn a new agent
- `pu kill --agent <agent_id>` — stop an agent
- `pu machine list` / `pu --machine <name> <command>` — work with a daemon on another computer over SSH (project commands need `--project <remote root>`; never auto-starts it)

## Conversation Monitoring

Prefer `pu read <agent_id> --json` for native messages and tool activity. Use `--limit 1..200` (default 50) and pass the returned `next_cursor` to `--since` for incremental reads. Restart without `--since` if the cursor is stale. Terminal workers return explicitly labeled terminal text. Missing identity or unsupported native data is an explicit error; never guess the latest transcript. Bind legacy sessions with `--session EXACT_ID` only when the ID is verified.

Check `pu read --help` before using it on older installations. CLI and daemon must both support protocol 8; do not restart active agents to force an upgrade. `pu logs` remains available for raw terminal diagnostics.
