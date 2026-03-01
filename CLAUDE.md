# PurePoint — Agent Instructions

## What is PurePoint?

An agent-first coding workspace. IDEs were built for humans writing code — PurePoint is built for a world where agents write code and humans direct the work. You describe what needs to happen; PurePoint sets up isolated workstations (git worktrees with live agents) and orchestrates parallel execution.

See `vision.md` for the full product vision. See `CONTEXT.md` for identity and naming.

## Tech Stack

- **Engine**: Rust (tokio async runtime, tonic gRPC, SQLite)
- **Desktop**: Swift/AppKit (macOS), terminal UI (Linux)
- **CLI**: `pu` command → gRPC to daemon
- **Config dir**: `.pu/`
- **Branch naming**: `pu/{name}`
- **Crates**: pu-core (domain logic), pu-daemon (gRPC server), pu-cli (thin client), pu-proto (protobuf)

## Reading Protocol

All specs live in `docs/` as markdown. Read what you need for the task at hand:

| Task | Read |
|---|---|
| Any PurePoint work (first time) | `docs/spec-system.md` — conventions, maturity levels, protocols |
| Implementing code | `docs/product/{domain}.md` + its listed Dependencies |
| TDD | `docs/process/tdd/rules.md` + `docs/process/tdd/per-language/{rust\|swift\|ts-js}.md` |
| Code review | `docs/process/code-review/rules.md` |
| Greenfield module | `docs/process/greenfield/rules.md` |
| Advancing a spec | `docs/process/spec-advancement/rules.md` |
| Task planning | `docs/process/task-planning/rules.md` |
| Finding dependencies | `docs/product/cross-reference-matrix.md` — maps domains to architecture, RPCs, tables, commands |

**Maturity gate**: If a product/architecture spec is at SEED or EXPLORING maturity, it needs research before implementation. See the spec advancement process to advance it.

NOTE: This manual protocol is temporary. PurePoint's daemon will handle context assembly automatically once implemented.
