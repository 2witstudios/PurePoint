# Module Structure

**Maturity: SEED**

## Context

PurePoint's codebase needs a clear module/package structure. The structure determines compilation boundaries, API surfaces between modules, and how the daemon, CLI, and API definitions relate to each other. Getting the boundaries right affects build times, testability, and how cleanly domain logic separates from transport and infrastructure.

## Open Questions

? [MOD-001] What is the right module granularity?
How many separate packages/crates/modules? A few large ones (core, daemon, cli) or many small ones (db, process-management, git, agent)? More modules = better parallelism and clearer APIs, but more ceremony and dependency management.

? [MOD-002] Where should API definitions live?
Options: in the main repo, in a separate package, or in a separate repo (for sharing across languages). How should API schema compilation work — at build time or pre-generated?

? [MOD-003] What language should PurePoint be built in?
Options: Rust, Go, TypeScript, Python, or a mix. Trade-offs: performance, ecosystem, development speed, cross-compilation, team familiarity.

## Design Directions

- CLI and daemon as separate binaries
- Core domain logic with no dependency on transport/API layer
- API definitions accessible to all client implementations
- Support for both fast iteration (debug) and optimized (release) builds
