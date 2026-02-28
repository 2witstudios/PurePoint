# Desktop App Integration

**Maturity: SEED**

## Context

PurePoint.app is a macOS desktop app (Swift/AppKit) that provides the primary visual interface — project tree sidebar, terminal views for agents, pane grid layout, dashboard, and configuration editors. With the daemon architecture, the app becomes a client: all state comes from the daemon, all mutations go through the API. The integration layer determines how the app communicates with the daemon, manages daemon lifecycle, and renders real-time agent activity.

## Open Questions

? [DESK-001] Should the app use grpc-swift, FFI to the Rust library, or XPC?
grpc-swift: clean separation, same API as remote clients, but adds a dependency and serialization overhead. FFI: direct Rust function calls via C ABI, fastest, but complex memory management and no streaming. XPC: macOS-native IPC, but only works on macOS and is more complex than gRPC. A leading option is grpc-swift but it needs validation of the Swift gRPC ecosystem maturity.

? [DESK-002] Should the daemon binary be embedded in the app bundle or installed separately?
Embedded: single download, app manages daemon lifecycle, version-locked. Separate: CLI and app can update independently, but version skew risk. One approach is embedding in the app bundle — but what about users who only want the CLI?

? [DESK-003] How should the app manage daemon lifecycle?
App starts daemon on launch? App starts daemon on first project open? App reuses existing daemon if already running (started by CLI)? What happens when the app quits — kill the daemon or leave it running for CLI? What about multiple app instances?

## Design Directions

- Maintain all current app capabilities (see CONTEXT.md Desktop App section)
- Work with both embedded daemon and standalone daemon (for CLI-only users)
- Graceful daemon connection loss handling (reconnect, show status)
- Sparkle auto-update support for both app and daemon
- Signed and notarized for distribution
