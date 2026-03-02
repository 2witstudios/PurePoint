# Distribution

**Maturity: EXPLORING**

## Context

PurePoint needs to ship to users as a self-contained product with no external runtime dependencies. The distribution model covers how the app, daemon, and CLI are packaged, installed, and updated.

## Decisions

! [DIST-001] Daemon embedded in app bundle. The `pu-engine` binary is compiled via a Run Script build phase in Xcode and placed at `Contents/MacOS/pu-engine` alongside the main app executable. Debug builds compile for the host architecture only; release builds create a universal binary (ARM64 + x86_64) via `lipo`. Code signed with the app's identity. DaemonLifecycle checks the app bundle first, then PATH, then ~/.cargo/bin (development fallback). No external runtime dependencies — the app is a single drag-and-drop download.

## Open Questions

? [DIST-002] What is the migration path for existing users?
Existing users may have legacy project directories and data. Should migration be automatic, an explicit command, or handled by the app?

? [DIST-003] How should auto-update work?
If the daemon is embedded in the app, updating the app updates the daemon too. But what if the daemon is running when the update happens? How are running processes handled during updates?

## Design Directions

- macOS as primary platform
- No external runtime dependencies in final product
- Support for multiple CPU architectures
- Signed and verified for distribution

## Research Notes

DIST-003 partially answered: updating the app updates the daemon because it's embedded. Running daemon during update: the app sends Shutdown before quit. If the daemon was started by CLI in standalone mode, the update only affects the bundled copy — the standalone binary in PATH is managed separately (e.g. cargo install).
