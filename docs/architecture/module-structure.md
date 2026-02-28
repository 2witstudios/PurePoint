# Module Structure

**Maturity: SEED**

## Context

PurePoint is a Rust workspace with multiple crates. The crate structure determines compilation boundaries, API surfaces between modules, and how the daemon, CLI, and proto definitions relate to each other. Getting the boundaries right affects build times, testability, and how cleanly domain logic separates from transport and infrastructure.

## Open Questions

? [MOD-001] What is the right crate granularity?
One approach proposes 4 crates: `pu-proto`, `pu-core`, `pu-daemon`, `pu-cli`. Should `pu-core` be further split? E.g., `pu-db` (SQLite layer), `pu-tmux` (tmux operations), `pu-git` (worktree management), `pu-agent` (agent lifecycle). More crates = better compilation parallelism and clearer APIs, but more ceremony and cross-crate dependency management.

? [MOD-002] Where should proto definitions live?
Options: `proto/` at workspace root, inside `pu-proto` crate, or in a separate repo (for sharing with Swift). If proto files are in the workspace, both the Rust build (tonic-build) and Swift build (grpc-swift-protobuf) need access. Should proto compilation happen at build time (build.rs) or be pre-generated and checked in?

## Design Directions

- Cargo workspace at repository root
- Proto definitions accessible to both Rust and Swift build systems
- `pu-cli` and `pu-daemon` as separate binaries
- `pu-core` with no dependency on gRPC/tonic (pure domain logic)
- Support for both debug (fast iteration) and release (optimized) profiles
