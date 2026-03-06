# Worktree Management

**Maturity: SEED** | ID Prefix: WT | Dependencies: none

## Purpose

Git worktree creation, tracking, merging, and cleanup. Each unit of work gets an isolated worktree on a `pu/{name}` branch.

## Conceptual Model

```
Worktree lifecycle: active → merging → merged | failed | cleaned
Branch naming: pu/{worktree-name}
Location: {project_root}/.pu/worktrees/{name}/
```

## Research Notes

**Rename and delete:** `Request::Rename` and `Request::DeleteWorktree` protocol handlers implemented in the engine. Sidebar context menus wire rename/delete into macOS client. Not yet exposed as CLI commands (protocol-only).

**Stale cleanup:** Stale worktree cleanup on spawn implemented in engine — removes orphaned worktree directories.

**Shared access:** Current implementation supports multiple agents per worktree (worktree has `agents: HashMap`).

## Open Questions

? [WT-001] How should worktree cleanup be triggered — on agent completion, on explicit user command, or on a TTL basis?
(Partial answer: stale cleanup on spawn is implemented. Explicit delete via sidebar context menu exists.)
