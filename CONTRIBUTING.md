# Contributing to PurePoint

## Quick start

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
| macOS | 26.1+ | (deployment target) |
| Rust | 1.88+ | `rustup` (auto-installed by `just setup`) |
| Xcode | 16.1+ | Mac App Store |
| just | any | `brew install just` |
| swift-format | any | `brew install swift-format` (optional) |
| cargo-deny | any | `cargo install cargo-deny` |

## Guides

| Guide | What it covers |
|---|---|
| [Architecture](docs/contributing/architecture.md) | System diagram, crate map, data flows |
| [Building](docs/contributing/building.md) | Rust CLI, macOS app, debug vs release |
| [Testing](docs/contributing/testing.md) | Running and writing tests |
| [Code Style](docs/contributing/code-style.md) | Formatting, linting, commit messages |

## Commit messages

Enforced by git hook. Format: `type[(scope)]: Description starting with capital`

Types: `feat`, `fix`, `chore`, `docs`, `refactor`, `test`, `ci`, `perf`, `style`, `build`

Max 72 characters, no trailing period, sentence case.

## Branch naming

Use `pu/{name}` for PurePoint branches.

## Pull requests

- One feature per PR
- All CI checks must pass (`just ci`)
- Follow the code style guide
