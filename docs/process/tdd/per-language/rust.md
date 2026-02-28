# TDD — Rust

Rust-specific TDD conventions for PurePoint's Rust codebase (pu-core, pu-daemon, pu-cli, pu-proto).

## Test Framework

- Built-in `#[cfg(test)]` module with `#[test]` attribute
- `cargo test` as the test runner
- `cargo test -p pu-core` for per-crate testing
- `cargo test -- --nocapture` for output visibility during debugging

## Test Organization

- **Unit tests**: In `#[cfg(test)] mod tests {}` at the bottom of each source file
- **Integration tests**: In `tests/` directory at crate root
- **Doc tests**: In `///` doc comments (use sparingly — only for public API examples)

## Assert Format (Rust Adaptation)

Rust doesn't have the `{ given, should, actual, expected }` assert function. Adapt the principle:

```rust
#[test]
fn given_new_user_should_create_account() {
    // given
    let credentials = Credentials::new("user@test.com", "password123");
    
    // when
    let result = create_account(&credentials);
    
    // then
    assert_eq!(result.unwrap().email, "user@test.com");
}
```

Rules:
- Name test functions as `given_{situation}_should_{behavior}`
- Use `// given`, `// when`, `// then` comments for structure
- Prefer `assert_eq!` and `assert_ne!` over bare `assert!`
- Use `assert!(matches!(value, Pattern))` for enum variants

## Mocking Strategy

- Prefer trait-based dependency injection over mocking frameworks
- Define traits for external dependencies (tmux, git, filesystem, SQLite)
- Implement test doubles as simple structs implementing the trait
- Use `mockall` crate only when trait-based DI is impractical
- For integration tests, use real dependencies (actual SQLite, actual git repos in temp dirs)

## Test Isolation

- Use `tempdir` (via `tempfile` crate) for filesystem tests
- Each test creates its own SQLite database (in-memory or temp file)
- No shared global state — no `lazy_static` test fixtures
- Use `#[serial]` from `serial_test` crate only when testing global resources (e.g., tmux server)

## Async Tests

- Use `#[tokio::test]` for async test functions
- Prefer `#[tokio::test(flavor = "current_thread")]` for deterministic behavior
- Use `tokio::time::pause()` for timer-dependent tests
