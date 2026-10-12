# Worktree Management

**Maturity: SEED** | ID Prefix: WT | Dependencies: none

## Purpose

Git worktree creation, tracking, merging, and cleanup. Each unit of work gets an isolated worktree on a `pu/{name}` branch.

## Conceptual Model

```
Worktree lifecycle: active → merging → merged | failed | cleaned
Branch naming: pu/{worktree-name}
Location: {project_root}/.pu/worktrees/{worktree-id}/  (e.g. .pu/worktrees/wt-abc12345/)
```

## Research Notes

**Rename and delete:** `Request::Rename` and `Request::DeleteWorktree` protocol handlers implemented in the engine. Sidebar context menus wire rename/delete into macOS client. CLI exposes worktree deletion via `pu clean --worktree <id>` (or `pu clean --all` for all worktrees), which sends `DeleteWorktree` to the daemon, kills associated agents, and removes the git branch.

**Delete operation sequence:**
1. Kill all agents running in the worktree
2. Remove git worktree directory
3. Delete local branch
4. Delete remote branch (if exists)
5. Remove worktree entry from manifest

**Stale cleanup:** Stale worktree cleanup on spawn implemented in engine — removes orphaned worktree directories.

**Build caches (implemented 2026-10-12):** Agents spawned or resumed in a worktree get `CARGO_TARGET_DIR=<worktree>/target`, overriding any inherited value, so their Rust builds are isolated per worktree and the cache is deleted with the worktree (`git worktree remove --force` takes untracked files). Root agents keep the project's own target dir. The daemon sweeps at startup and every 30 minutes: it removes `purepoint-*-target` Cargo caches in `/tmp` and `$TMPDIR` (the old per-agent convention) once nothing has been written to them for 6 hours; when free space on `/` is under 20 GiB the idle limit drops to 30 minutes and idle `<worktree>/target` caches of registered projects are removed too. A dir is only removed if it carries Cargo's `CACHEDIR.TAG` or `.rustc_info.json`, is not a symlink, and no process holds an flock on any of its `.cargo-lock` files.

**Shared access:** Current implementation supports multiple agents per worktree (worktree has `agents: IndexMap`, insertion-order preserved).

## Open Questions

? [WT-001] How should worktree cleanup be triggered — on agent completion, on explicit user command, or on a TTL basis?
(Partial answer: stale cleanup on spawn is implemented. Explicit delete via sidebar context menu exists.)
