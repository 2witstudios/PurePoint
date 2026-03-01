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

## Open Questions

? [WT-001] How should worktree cleanup be triggered — on agent completion, on explicit user command, or on a TTL basis?

? [WT-002] Should worktrees support shared access (multiple agents in one worktree) or strictly one-agent-per-worktree?
