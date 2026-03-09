# Testing

How to run and write tests for PurePoint.

## Running tests

### Rust

```sh
just test              # all Rust tests
cargo test -p pu-core  # single crate
cargo test -p pu-engine
cargo test -p pu-cli
```

`just test` runs `cargo test --all-targets` across all crates.

### Swift

```sh
just test-app          # macOS app tests (unsigned)
```

Runs `xcodebuild test` with signing disabled.

### Full CI locally

```sh
just ci-rust           # fmt-check + lint + test + deny
just ci                # ci-rust + build-app + test-app
```

### Filtering tests

```sh
cargo test -p pu-core given_manifest   # name substring match
cargo test -p pu-engine -- --nocapture # show stdout
```

## Test structure

### Unit tests

Each module has a `#[cfg(test)] mod tests` block at the bottom:

```
crates/pu-core/src/
  types.rs          # serialization round-trips, status mapping
  protocol.rs       # request/response serialization (all 49 variants)
  manifest.rs       # file I/O with tempdir
  config.rs         # YAML parsing, default generation
  template.rs       # parse, render, variable extraction
  schedule_def.rs   # YAML parsing, next_occurrence calculation
  trigger_def.rs    # YAML parsing, event/action variants
  validation.rs     # name validation, path traversal protection
  id.rs             # ID generation format

crates/pu-engine/src/
  output_buffer.rs  # circular buffer, idle detection, ANSI stripping
  engine.rs         # agent spawning, state transitions
  agent_monitor.rs  # effective status computation
  gate.rs           # gate evaluation (async)
  daemon_lifecycle.rs # PID file management
  attach_handler.rs # APC escape parsing
  git.rs            # worktree operations
```

### Integration tests

`crates/pu-engine/tests/integration.rs` — 11 tests that start a real IPC server, send requests, and verify responses. Uses a `TestHarness` that manages server lifecycle:

```rust
struct TestHarness {
    _tmp: TempDir,
    sock: std::path::PathBuf,
    project: std::path::PathBuf,
    handle: tokio::task::JoinHandle<()>,
}
```

### Swift tests

`apps/purepoint-macos/purepoint-macosTests/` — uses Apple's `Testing` framework (`@Test`, `#expect`):

| File | What it tests |
|---|---|
| `ChatStateTests.swift` | Point Guard conversation state |
| `DaemonClientParseTests.swift` | NDJSON response parsing |
| `TranscriptParserTests.swift` | Agent transcript extraction |
| `HexEncodingTests.swift` | Hex byte encoding/decoding |
| `AgentsHubModelsTests.swift` | Agent def / template models |
| `AgentsHubStateTests.swift` | Hub tab state management |
| `ActiveProjectRoutingTests.swift` | Project selection routing |
| `ClaudeConversationIndexTests.swift` | Conversation index parsing |

## Writing tests

### Naming convention

All tests use **Given/Should** naming:

**Rust** (snake_case):
```rust
#[test]
fn given_manifest_should_write_and_read_back_identical() { ... }

#[tokio::test]
async fn given_passing_gate_should_return_passed() { ... }
```

**Swift** (camelCase):
```swift
@Test func givenNewConversationShouldClearMessagesAndSessionId() { ... }
```

The pattern is: `given_<precondition>_should_<expected_behavior>`. Every test follows this — no exceptions.

### Filesystem isolation

Use `tempfile::TempDir` for any test that touches the filesystem:

```rust
use tempfile::TempDir;

#[test]
fn given_config_should_load_from_file() {
    let tmp = TempDir::new().unwrap();
    let config_path = tmp.path().join("config.yaml");
    std::fs::write(&config_path, "default_agent_type: claude").unwrap();
    // test against config_path...
}
```

`TempDir` auto-cleans on drop. Never write to real paths in tests.

### Async tests

Use `#[tokio::test]` for async tests. Integration tests use single-threaded runtime for determinism:

```rust
#[tokio::test(flavor = "current_thread")]
async fn given_full_lifecycle_should_init_status_and_shutdown() {
    let harness = TestHarness::new().await;
    // ...
    harness.shutdown().await;
}
```

### Mocks and doubles

Hand-written — no mocking libraries.

**Rust**: Helper functions that build test data:
```rust
fn make_gate_trigger(cmd: &str, inject: Option<&str>) -> TriggerAction { ... }
```

**Swift**: Protocol-conforming mock structs:
```swift
final class MockClaudeProcess: ClaudeProcessProvider, @unchecked Sendable {
    var events: [StreamEvent] = []
    var cancelCalled = false
    // ...
}
```

### Round-trip testing

Common pattern for serialization types — serialize then deserialize and compare:

```rust
#[test]
fn given_all_agent_statuses_should_round_trip_json() {
    for status in [AgentStatus::Streaming, AgentStatus::Waiting, AgentStatus::Broken] {
        let json = serde_json::to_string(&status).unwrap();
        let parsed: AgentStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, status);
    }
}
```

### What to test

- **pu-core**: Serialization round-trips, YAML parsing, file I/O, validation edge cases
- **pu-engine**: Status computation, buffer behavior, gate evaluation, IPC request handling
- **Swift**: State transitions, NDJSON parsing, model encoding/decoding
