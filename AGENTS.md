# PurePoint — Agent Instructions

## What is PurePoint?

An agent-first coding workspace. IDEs were built for humans writing code — PurePoint is built for a world where agents write code and humans direct the work. You describe what needs to happen; PurePoint sets up isolated workstations (git worktrees with live agents) and orchestrates parallel execution.

See `vision.md` for the full product vision.

## Key Conventions

- **Name**: PurePoint ([purepoint.dev](https://purepoint.dev))
- **CLI**: `pu` command
- **Config dir**: `.pu/` (replaces the old ppg / pure-point-guard / `.ppg/`)
- **Branch naming**: `pu/{name}`

## Reading Protocol

All specs live in `docs/` as markdown. Read what you need for the task at hand:

| Task | Read |
|---|---|
| Any PurePoint work (first time) | `docs/spec-system.md` — conventions, maturity levels, protocols |
| Implementing code | `docs/product/{domain}.md` + its listed Dependencies |
| TDD | `docs/process/tdd/rules.md` + `docs/process/tdd/per-language/{lang}.md` |
| Code review | `docs/process/code-review/rules.md` |
| Greenfield module | `docs/process/greenfield/rules.md` |
| Advancing a spec | `docs/process/spec-advancement/rules.md` |
| Task planning | `docs/process/task-planning/rules.md` |
| Finding dependencies | `docs/product/cross-reference-matrix.md` — maps domains to architecture, API ops, stored data, commands |

**Maturity gate**: If a product/architecture spec is at SEED or EXPLORING maturity, it needs research before implementation. See the spec advancement process to advance it.

NOTE: This manual protocol is temporary. PurePoint's daemon will handle context assembly automatically once implemented.

## macOS App Build Safety

**Do NOT run `xcodebuild` from an agent session.** The `purepoint-macos` build phase runs `cargo build` and copies the `pu-engine` binary to `~/.cargo/bin/` and `pu` to `~/.pu/bin/` (see the build script in the Xcode project). If the `pu-engine` daemon managing this agent session is running, overwriting its binary invalidates the code signature on the mapped executable, causing macOS to SIGKILL the process (exit 137) — killing your own session.

**Rules:**
- Never run `xcodebuild build`, `xcodebuild test`, or `swift build` for the macOS app from an agent session
- Ask the user to build and run tests in Xcode directly
- In worktrees: all builds share the same `~/.pu/bin/` and `~/.cargo/bin/` destinations — concurrent installs from multiple worktrees will race on these binaries and can kill any running daemon

## Rust Build Caches

The daemon sets `CARGO_TARGET_DIR=<worktree>/target` for every agent it spawns in a worktree, so each worktree already has its own Cargo build cache and plain `cargo build` / `cargo test` is isolated from other agents.

- Do not override `CARGO_TARGET_DIR` or pass `--target-dir` (in particular, no `/tmp/purepoint-*-target` dirs). The worktree cache is deleted with the worktree; anything elsewhere is left behind and fills the disk.
- The daemon sweeps abandoned `/tmp/purepoint-*-target` caches once nothing has built into them for 6 hours (30 minutes when free disk is under 20 GiB, when it also drops idle caches of worktrees that still exist). A cache with a build in progress is never removed.
- Check `df -h /` before long builds.
