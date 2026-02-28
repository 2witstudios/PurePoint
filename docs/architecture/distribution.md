# Distribution

**Maturity: SEED**

## Context

PurePoint needs to ship to users as a self-contained product with no external runtime dependencies. The distribution model covers how the app, daemon, and CLI are packaged, installed, and updated. A leading approach is a single DMG download containing PurePoint.app with the Rust daemon embedded, Sparkle auto-update for both app and daemon, and an optional CLI available via symlink or Homebrew.

## Open Questions

? [DIST-001] How should the universal binary be built?
Rust supports cross-compilation for both x86_64-apple-darwin and aarch64-apple-darwin. Options: build two separate binaries and use `lipo` to create a universal binary, or build with cargo's target triple support. How does this interact with the Swift app's universal binary build? Should the Rust binary be built as part of the Xcode build process or separately?

? [DIST-002] What is the migration path for existing users?
Existing users may have legacy project directories, JSON manifests, templates, swarms, etc. PurePoint may need a migration tool to convert these to the new format (SQLite, `.pu/` directory structure). Should this be automatic on first `pu` run, a separate `pu migrate` command, or handled by the app?

? [DIST-003] How should Sparkle auto-update work with an embedded daemon?
Sparkle updates the app bundle. If the daemon binary is in the bundle, Sparkle updates it too. But what if the daemon is running when the update happens? Need to: stop daemon, replace binary, restart daemon. Should the app handle this, or should Sparkle's pre/post-install scripts manage it?

## Design Directions

- macOS 13+ (Ventura and later)
- Signed and notarized for Gatekeeper
- No Node.js or npm dependency in final product
- Support for both Apple Silicon and Intel Macs
- Sparkle handling both app and daemon updates atomically
