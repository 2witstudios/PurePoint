# Contributing to PurePoint

Quick start for contributors.

## Setup

```sh
git clone https://github.com/2witstudios/purepoint.git
cd purepoint
just setup        # git hooks, Rust toolchain, swift-format check
just ci-rust      # fmt + lint + test + deny
just build-app    # build macOS app
just test-app     # run Swift tests
```

## Prerequisites

| Requirement | Version | Install |
|---|---|---|
| macOS | 26.1+ | (deployment target for app) |
| Rust | 1.88+ | `rustup` (auto-installed by `just setup`) |
| Xcode | 16.1+ | Mac App Store |
| just | any | `brew install just` |
| swift-format | any | `brew install swift-format` (optional) |
| cargo-deny | any | `cargo install cargo-deny` (for dependency audit) |

## Build commands

| Command | What it does |
|---|---|
| `just fmt` | Format all Rust code |
| `just fmt-swift` | Format all Swift code |
| `just lint` | Run clippy with warnings as errors |
| `just test` | Run all Rust tests |
| `just deny` | Check dependency licenses and advisories |
| `just ci-rust` | Full Rust CI: fmt + lint + test + deny |
| `just build-app` | Build macOS app (unsigned) |
| `just test-app` | Run macOS app tests |
| `just ci` | Full CI: ci-rust + build-app + test-app |

## Detailed guides

- [Architecture](architecture.md) -- system diagram, crate map, data flow
- [Building](building.md) -- Rust CLI, macOS app, debug vs release
- [Testing](testing.md) -- running and writing tests
- [Code Style](code-style.md) -- formatting, linting, commit messages

## Branch naming

Use `pu/{name}` for PurePoint branches.

## Commit messages

Enforced by git hook. Format: `type[(scope)]: Description starting with capital`

Types: `feat`, `fix`, `chore`, `docs`, `refactor`, `test`, `ci`, `perf`, `style`, `build`

Examples:
```
feat: Add workspace pulse command
fix(cli): Resolve manifest parsing error
docs: Update CLI reference with missing commands
```

Max 72 characters, no trailing period, sentence case.

## Pull requests

- One feature per PR
- All CI checks must pass
- Follow the code review process in `docs/process/code-review/`
