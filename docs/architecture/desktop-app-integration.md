# Desktop App Integration

**Maturity: DECIDED**

## Context

The desktop app provides the primary visual interface — project tree sidebar, terminal views for agents, pane grid layout, dashboard, and configuration editors. Initially operates without a daemon, reading manifest.json directly and talking to tmux. Service abstracted behind a protocol for future daemon swap.

## Decisions

! [DESK-001] Direct tmux commands + manifest.json file watching. App reads `.pu/manifest.json` for workspace state, uses tmux grouped sessions for terminal display. Service abstracted behind `WorkspaceService` protocol for future daemon swap — when the daemon is implemented, a `DaemonWorkspaceService` replaces `TmuxWorkspaceService` without changing any view code.

! [DESK-002] No daemon initially. App reads `.pu/manifest.json` directly and communicates with tmux via shell commands. The daemon will be a future addition.

! [DESK-003] N/A initially. Tmux sessions persist independently of the desktop app. The app attaches/detaches viewers without affecting agent processes.

! [DESK-004] Native macOS. SwiftUI as primary framework + NSViewRepresentable bridges for SwiftTerm terminal views. AppKit only where SwiftUI lacks capability (NSSplitView for programmatic ratio control, NSEvent monitoring for scroll interception).

## Design Directions

- WorkspaceService protocol as abstraction boundary
- Graceful degradation when manifest doesn't exist
- File watching with debounce for live updates
