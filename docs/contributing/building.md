# Building

How to build PurePoint's Rust binaries and macOS app.

## Rust CLI and engine

Build all crates:

```sh
cargo build                # debug
cargo build --release      # release
```

Build a specific crate:

```sh
cargo build -p pu-cli
cargo build -p pu-engine
cargo build -p pu-core     # library only
```

Debug binaries output to `target/debug/pu` and `target/debug/pu-engine`.

### Running the CLI directly

```sh
cargo run -p pu-cli -- spawn "test" --name test
cargo run -p pu-cli -- status
cargo run -p pu-cli -- health
```

### Running the daemon directly

```sh
cargo run -p pu-engine
cargo run -p pu-engine -- --managed    # managed mode (for app)
cargo run -p pu-engine -- --socket /tmp/test.sock
```

## macOS app

Build the app (unsigned, for local development):

```sh
just build-app
```

This runs `xcodebuild build` with signing disabled.

### How the Xcode build works

The Xcode project includes a "Build Rust Binaries" Run Script phase that runs **before** Swift compilation:

1. Adds `$HOME/.cargo/bin` to PATH (Xcode strips default PATH)
2. **Debug**: `cargo build -p pu-engine -p pu-cli` (host architecture only)
3. **Release**: Builds both `aarch64-apple-darwin` and `x86_64-apple-darwin`, merges with `lipo`
4. Code signs Rust binaries with the app identity
5. Copies `SKILL.md` to `Resources/pu-skill.md`

Binary locations in app bundle:
- `Contents/MacOS/pu-engine` -- daemon binary
- `Contents/MacOS/pu` -- CLI binary
- `Contents/Resources/pu-skill.md` -- Claude Code skill

### Development workflow

Typical edit-build-test cycle for Rust changes:

```sh
# Edit Rust code
just test           # Run Rust tests
just build-app      # Rebuild app (includes Rust build phase)
# Launch app from Xcode or build output
```

For Swift-only changes, use Xcode directly.

## Release builds

### Archive and export

```sh
just archive        # Archive for release
just export         # Export from archive
```

### Full release pipeline (CI)

The release CI workflow (`.github/workflows/release.yml`) triggered by `v*` tags:

1. Build universal Rust binaries (arm64 + x86_64)
2. Archive Xcode project with Developer ID signing
3. Export and notarize with Apple
4. Create DMG and sign with Sparkle
5. Create GitHub Release with DMG attached
6. Generate Sparkle appcast XML

## Toolchain

| Tool | Config file | Version |
|---|---|---|
| Rust | `rust-toolchain.toml` | 1.88 |
| rustfmt | `rustfmt.toml` | edition 2024 |
| clippy | `clippy.toml` | MSRV 1.88 |
| Xcode | project.pbxproj | 16.1+ (objectVersion 77) |
| Swift | Xcode project | 5.0 |

### SPM dependencies

| Package | Version | Purpose |
|---|---|---|
| SwiftTerm | 1.11.2 | Terminal emulation |
| Sparkle | 2.9.0 | Auto-update framework |

### App entitlements

- App Sandbox: **disabled** (needed for PTY access and worktree management)
- Hardened Runtime: **enabled**
