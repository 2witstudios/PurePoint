# Data Model

**Maturity: SEED** | ID Prefix: DM

## Purpose

Defines the core entities, relationships, and state machines that make up PurePoint's domain model. Everything flows from this: the SQLite schema, gRPC messages, CLI output, and dashboard state.

## Conceptual Model

```
Project
  Sessions (units of work)
    Worktrees (git worktree on branch pu/{name})
      Agents (claude, codex, opencode, terminal)
        Live output stream
        Events (spawned, prompt sent, tool used, completed, failed)
        Summaries (auto-generated)
        Result (final output/artifacts)
      Memory (per-worktree context)
    Session memory (decisions, outcomes, patterns)
  Project memory (cross-session knowledge)
```

## Interfaces

```
AgentStatus = running | idle | exited | gone
WorktreeStatus = active | merging | merged | failed | cleaned
AgentEntry { id, name, agentType, status, tmuxTarget, prompt, exitCode, sessionId }
WorktreeEntry { id, name, path, branch, baseBranch, status, tmuxWindow, prUrl, agents, createdAt, mergedAt }
```
