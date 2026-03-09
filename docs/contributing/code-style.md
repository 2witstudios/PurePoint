# Code Style

Formatting, linting, and commit conventions.

## Formatting

### Rust

```sh
just fmt          # format all Rust code
just fmt-check    # check without modifying (CI uses this)
```

Config in `rustfmt.toml`:

```toml
edition = "2024"
```

All other settings are rustfmt defaults (4-space indent, 100-char lines).

### Swift

```sh
just fmt-swift    # format all Swift code
```

Config in `.swift-format`:

| Setting | Value |
|---|---|
| Indentation | 4 spaces |
| Line length | 120 characters |
| Respects existing line breaks | yes |
| Max consecutive blank lines | 1 |

Disabled lint rules: `AllPublicDeclarationsHaveDocumentation`, `NeverForceUnwrap`, `NeverUseForceTry`, `NeverUseImplicitlyUnwrappedOptionals`, `AlwaysUseLiteralForEmptyCollectionInit`.

### Editor config

`.editorconfig` applies to all editors:

| File type | Indent | Notes |
|---|---|---|
| `*.rs` | 4 spaces | |
| `*.swift` | 4 spaces | |
| `*.yml`, `*.yaml`, `*.toml` | 2 spaces | |
| All files | — | LF line endings, final newline, trim trailing whitespace, UTF-8 |

## Linting

### Clippy

```sh
just lint         # clippy with warnings as errors
```

Runs `cargo clippy --all-targets` with `RUSTFLAGS="-D warnings"`. All warnings are errors in CI.

Config in `clippy.toml`:

```toml
msrv = "1.88"
```

### Dependency audit

```sh
just deny         # check licenses, advisories, bans, sources
```

Config in `deny.toml`. Allowed licenses: MIT, Apache-2.0, BSD-2-Clause, BSD-3-Clause, BSL-1.0, ISC, Unicode-3.0, Unlicense, Zlib. Unknown registries and git sources are denied.

## Git hooks

Installed by `just setup`. Located in `.githooks/`.

### Pre-commit

Three checks, each skippable individually:

| Check | What it does | Skip with |
|---|---|---|
| `rust-fmt` | `cargo fmt --all -- --check` | `SKIP_HOOKS=rust-fmt` |
| `swift-fmt` | `swift-format lint --strict` on staged `.swift` files | `SKIP_HOOKS=swift-fmt` |
| `hygiene` | Blocks files >1MB, merge conflict markers, secret patterns | `SKIP_HOOKS=hygiene` |

Secret patterns detected: AWS keys, private keys (RSA/EC/DSA/OpenSSH), GitHub tokens, Slack tokens, generic API keys, password/api_key assignments.

Skip all checks: `SKIP_HOOKS=all git commit ...`

### Commit message

Format: `type[(scope)]: Description starting with capital`

| Rule | Value |
|---|---|
| Types | `feat`, `fix`, `chore`, `docs`, `refactor`, `test`, `ci`, `perf`, `style`, `build` |
| Max length | 72 characters |
| Case | Sentence case (first letter uppercase) |
| Trailing period | Not allowed |
| Scope | Optional, alphanumeric + underscore + hyphen |

Examples:
```
feat: Add workspace pulse command
fix(cli): Resolve manifest parsing error
docs: Update CLI reference with missing commands
```

Exceptions: merge commits, revert commits, fixup/squash commits.

## Branch naming

Use `pu/{name}` for PurePoint branches.

## CI pipeline

`just ci-rust` runs the full Rust check suite: `fmt-check` + `lint` + `test` + `deny`.

`just ci` adds: `build-app` + `test-app`.

Both match what runs in GitHub Actions (`.github/workflows/`).
