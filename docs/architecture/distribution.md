# Distribution

**Maturity: SEED**

## Context

PurePoint needs to ship to users as a self-contained product with no external runtime dependencies. The distribution model covers how the app, daemon, and CLI are packaged, installed, and updated.

## Open Questions

? [DIST-001] How should the product be packaged for distribution?
Options depend on platform decisions. Trade-offs: single download vs separate components, universal binaries, installer vs drag-and-drop.

? [DIST-002] What is the migration path for existing users?
Existing users may have legacy project directories and data. Should migration be automatic, an explicit command, or handled by the app?

? [DIST-003] How should auto-update work?
If the daemon is embedded in the app, updating the app updates the daemon too. But what if the daemon is running when the update happens? How are running processes handled during updates?

## Design Directions

- macOS as primary platform
- No external runtime dependencies in final product
- Support for multiple CPU architectures
- Signed and verified for distribution
