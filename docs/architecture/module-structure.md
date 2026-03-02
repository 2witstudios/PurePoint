# Module Structure

**Maturity: DECIDED**

## Context

PurePoint's codebase needs a clear module/package structure. The structure determines compilation boundaries, API surfaces between modules, and how the daemon, CLI, and API definitions relate to each other. Getting the boundaries right affects build times, testability, and how cleanly domain logic separates from transport and infrastructure.

## Decisions

! [MOD-001] Three crates: `pu-core` (shared types, no runtime deps), `pu-engine` (daemon binary), `pu-cli` (client binary) — balances separation of concerns with minimal ceremony. `pu-core` has zero async/runtime dependencies, making it lightweight to compile and usable by any future client. `pu-engine` depends on `pu-core` + tokio + nix for the daemon. `pu-cli` depends on `pu-core` + tokio + clap for the CLI. Workspace root `Cargo.toml` defines shared dependency versions. Implemented in `Cargo.toml` (workspace members: `crates/pu-core`, `crates/pu-engine`, `crates/pu-cli`).

**`pu-core` modules:** `config`, `error`, `id`, `manifest`, `paths`, `protocol`, `types` — all public, no re-export flattening (callers use `pu_core::protocol::Request`, etc.).

! [MOD-002] Protocol types in `pu-core::protocol`, shared by both binaries via Rust serde — no code generation, no separate schema repo. Request/Response enums with `#[serde(tag = "type", rename_all = "snake_case")]` for tagged-union JSON encoding. Domain types in `pu-core::types` with `#[serde(rename_all = "camelCase")]` for macOS app compatibility. This keeps API definitions in-repo, type-safe at compile time, and avoids build-time codegen complexity. Implemented in `pu-core/src/protocol.rs` and `pu-core/src/types.rs`.

! [MOD-003] Rust, edition 2024, MSRV 1.88 — Rust chosen for: memory safety without GC (critical for long-running daemon), excellent async ecosystem (tokio), strong type system for protocol correctness, single-binary deployment, and cross-compilation support. Edition 2024 for latest language features. MSRV 1.88 pinned in workspace `Cargo.toml` via `rust-version`. Implemented in `Cargo.toml` (`[workspace.package]` edition and rust-version).

## Design Directions

- CLI and daemon as separate binaries (two `[[bin]]` targets in separate crates)
- Core domain logic with no dependency on transport/API layer (`pu-core` has no tokio dep)
- API definitions accessible to all client implementations (via `pu-core` crate)
- Workspace-level dependency version management (`[workspace.dependencies]`: serde, serde_json, thiserror, chrono, tokio)
- Support for both fast iteration (debug) and optimized (release) builds
