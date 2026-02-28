# Worktree Management

**Maturity: SEED** | ID Prefix: WT

## Purpose

Git worktree creation, tracking, merging, and cleanup. Each unit of work gets an isolated worktree on a `pu/{name}` branch.

## Conceptual Model

```
WorktreeStatus: active → merging → merged | failed | cleaned
Branch naming: pu/{worktree-name}
Location: {project_root}/.pu/worktrees/{name}/
```

## Interfaces

```
WorktreeEntry { id, name, path, branch, baseBranch, status, tmuxWindow, prUrl, agents, createdAt, mergedAt }
WorktreeStatus = active | merging | merged | failed | cleaned
```
