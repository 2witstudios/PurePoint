# Desktop App Integration

**Maturity: DECIDED**

## Context

The desktop app provides the primary visual interface — project tree sidebar, terminal views for agents, pane grid layout, dashboard, and configuration editors. Connects to the daemon for workspace state and agent terminal streaming via IPC.

## Decisions

! [DESK-001] Daemon-based architecture. `DaemonWorkspaceService` reads workspace state via daemon IPC (Unix socket). Terminal views connect to agent PTYs via the `Attach`/`Output` protocol — the daemon streams live output and accepts input without the app needing direct process access. Service abstracted behind `WorkspaceService` protocol so view code is decoupled from the communication layer.

! [DESK-002] Daemon is the communication layer. App connects to the daemon's Unix socket for all workspace operations (status, spawn, kill, logs). No direct process management or shell commands.

! [DESK-003] App-managed daemon lifecycle. The app launches the daemon with `--managed` on startup and sends `Request::Shutdown` on quit — all agents stop and their state is saved to the manifest for restore on next launch. In standalone CLI mode (without `--managed`), the daemon writes a PID file and agents persist independently. The daemon owns the PTY master fds in both modes; the app only attaches/detaches terminal viewers via IPC.

! [DESK-004] Native macOS. SwiftUI as primary framework + NSViewRepresentable bridges for SwiftTerm terminal views. AppKit only where SwiftUI lacks capability (NSSplitView for programmatic ratio control, NSEvent monitoring for scroll interception).

## Design Directions

- WorkspaceService protocol as abstraction boundary
- Graceful degradation when daemon is unreachable
- File watching with debounce for live updates
