# Greenfield Rules

Imperative instructions for creating new modules, crates, or components in PurePoint.

## Before You Start

1. **Read the Architecture page** for the relevant domain — understand constraints and decisions
2. **Read the Product page** for the relevant domain — understand what it needs to do
3. **Check Module Structure** (Architecture) — understand crate boundaries and dependency rules

## New Rust Crate Checklist

1. Create the crate directory under `crates/`
2. Add `Cargo.toml` with workspace inheritance: `[package]` name, `[dependencies]` from workspace
3. Add to workspace `members` in root `Cargo.toml`
4. Create `src/lib.rs` (for library crates) or `src/main.rs` (for binary crates)
5. Define the public API — what types and functions are exported
6. Write the first test before the first implementation
7. Run `cargo check -p {crate-name}` to verify compilation
8. Add crate to dependent crates' `Cargo.toml` if needed

## New Module (within a crate) Checklist

1. Create `src/{module_name}.rs` or `src/{module_name}/mod.rs`
2. Add `pub mod {module_name};` to `lib.rs`
3. Define the public interface (traits, structs, functions)
4. Write tests in `#[cfg(test)] mod tests {}` at the bottom
5. Follow existing patterns in the crate for error handling, naming, and structure

## Conventions

- **Naming**: snake_case for modules and files, PascalCase for types, snake_case for functions
- **Error handling**: Define domain-specific error types using `thiserror`; use `anyhow` only at binary boundaries
- **Dependencies**: `pu-core` must NOT depend on `pu-daemon` or `pu-cli`; `pu-proto` is a leaf dependency
- **Tests**: Colocated in the same file; integration tests in `tests/` at crate root
- **Documentation**: `///` doc comments on all public items; `//!` module-level docs in `lib.rs`

## Constraints

- No circular dependencies between crates
- `pu-core` has no dependency on gRPC/tonic (pure domain logic)
- New crates must compile independently (`cargo check -p {crate}`)
- Follow TDD — write the test first, then the implementation
