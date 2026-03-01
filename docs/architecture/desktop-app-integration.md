# Desktop App Integration

**Maturity: SEED**

## Context

The desktop app provides the primary visual interface — project tree sidebar, terminal views for agents, pane grid layout, dashboard, and configuration editors. With the daemon architecture, the app becomes a client: all state comes from the daemon, all mutations go through the API. The integration layer determines how the app communicates with the daemon, manages daemon lifecycle, and renders real-time agent activity.

## Open Questions

? [DESK-001] How should the desktop app communicate with the daemon?
Options: API client (same protocol as CLI), FFI to a shared library, OS-native IPC, or a combination. Trade-offs: separation, performance, streaming support, platform portability.

? [DESK-002] Should the daemon be embedded in the app or installed separately?
Embedded: single download, version-locked. Separate: CLI and app update independently, but version skew risk. Both approaches have implications for CLI-only users.

? [DESK-003] How should the app manage daemon lifecycle?
When does the app start the daemon? What happens when the app quits — kill the daemon or leave it running? What about multiple app instances or concurrent CLI usage?

? [DESK-004] What platform and framework should the desktop app use?
Options: native per-platform (Swift/AppKit for macOS), cross-platform (Electron, Tauri, Flutter), or terminal UI. Trade-offs: native feel, development cost, platform reach.

## Design Directions

- Work with both embedded daemon and standalone daemon
- Graceful daemon connection loss handling
- Auto-update support for both app and daemon
