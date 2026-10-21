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
5. Copies `plugin/` directory to `Resources/pu-plugin/`

Binary locations in app bundle:
- `Contents/MacOS/pu-engine` -- daemon binary
- `Contents/MacOS/pu` -- CLI binary
- `Contents/Resources/pu-plugin/` -- Claude Code plugin (skills + commands)

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

The release CI workflow (`.github/workflows/release.yml`) runs on a `v*` tag push:

1. `build-macos` archives the universal app ad-hoc signed, with no secrets and a read-only token. `linux` builds the static musl tarballs the same way.
2. `sign-macos`, in the protected `release` environment, re-signs the app inside-out with Developer ID (`.github/scripts/sign-macos-app.sh`), notarizes and staples it, builds the DMG, signs, notarizes and staples the DMG, and signs it for Sparkle. Sparkle's `sign_update` and `create-dmg` are downloaded at pinned versions and checked against committed SHA-256 sums.
3. `publish` creates the GitHub release with the DMG and Linux tarballs in one step, then updates the Sparkle appcast on the `appcast` release.

A failed job can be re-run from the Actions page. The release is created only once every build is signed, so a re-run never replaces published assets. See DIST-005 in `docs/architecture/distribution.md` for the security model and the repository settings it relies on.

## Toolchain

| Tool | Config file | Version |
|---|---|---|
| Rust | `rust-toolchain.toml` | 1.89 |
| rustfmt | `rustfmt.toml` | edition 2024 |
| clippy | `clippy.toml` | MSRV 1.89 |
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
