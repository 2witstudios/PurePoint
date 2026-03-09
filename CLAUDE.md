# PurePoint — Agent Instructions

## What is PurePoint?

An agent-first coding workspace. IDEs were built for humans writing code — PurePoint is built for a world where agents write code and humans direct the work. You describe what needs to happen; PurePoint sets up isolated workstations (git worktrees with live agents) and orchestrates parallel execution.

See `vision.md` for the full product vision. See `CONTEXT.md` for identity and naming.

## Key Conventions

- **CLI**: `pu` command
- **Config dir**: `.pu/`
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

**Do NOT run `xcodebuild` from Claude Code.** The `purepoint-macos` build phase runs `cargo build` and copies the `pu-engine` binary to `~/.cargo/bin/` and `pu` to `~/.pu/bin/` (see the build script in the Xcode project). If the `pu-engine` daemon managing this agent session is running, overwriting its binary invalidates the code signature on the mapped executable, causing macOS to SIGKILL the process (exit 137) — killing your own session.

**Rules:**
- Never run `xcodebuild build`, `xcodebuild test`, or `swift build` for the macOS app from an agent session
- Ask the user to build and run tests in Xcode directly
- In worktrees: all builds share the same Cargo target directory and the same `~/.pu/bin/` and `~/.cargo/bin/` destinations — concurrent builds from multiple worktrees will race on these binaries and can kill any running daemon
