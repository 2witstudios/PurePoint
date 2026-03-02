# Swift App Inventory

Source map of the macOS desktop app (`apps/purepoint-macos/purepoint-macos/`).

## App Lifecycle

| File | Purpose |
|---|---|
| purepoint_macosApp.swift | SwiftUI app entry point |
| ContentView.swift | Root content view (sidebar + detail layout) |

## Services

| File | Purpose |
|---|---|
| DaemonClient.swift | NDJSON-over-Unix-socket client for daemon IPC |
| DaemonLifecycle.swift | Daemon auto-start, health check, graceful shutdown |
| DaemonWorkspaceService.swift | WorkspaceService implementation backed by daemon IPC |
| DaemonAttachSession.swift | Streaming attach session for live PTY output |
| WorkspaceService.swift | Protocol defining workspace operations |
| ManifestWatcher.swift | DispatchSource file watcher on .pu/manifest.json (triggers daemon refresh) |
| ShellUtilities.swift | Shell command execution utilities |

## Models

| File | Purpose |
|---|---|
| AgentStatus.swift | Agent status enum (Spawning, Running, Idle, Completed, Failed, Killed, Lost) |
| ManifestModel.swift | Manifest JSON decoding (mirrors pu-core Rust types) |
| SidebarItem.swift | Sidebar tree item model |
| WorkspaceModel.swift | Workspace state model |

## State

| File | Purpose |
|---|---|
| AppState.swift | Global app state (ObservableObject) |
| GridState.swift | Pane grid layout state |

## Views — Terminal

| File | Purpose |
|---|---|
| TerminalPaneView.swift | SwiftTerm terminal pane wrapper |
| ScrollableTerminal.swift | Scrollable terminal container |
| TerminalContainerView.swift | Terminal container with toolbar and status |
| TerminalViewCache.swift | Terminal view cache (hide/show, LRU eviction) |

## Views — Pane Grid

| File | Purpose |
|---|---|
| PaneGridView.swift | Pane grid system (split layout, up to 6 terminals) |
| PaneSplitNode.swift | Recursive binary split node |
| PaneCellView.swift | Individual pane cell in grid |
| NSSplitViewRepresentable.swift | AppKit NSSplitView bridge for SwiftUI |
| GridLayoutPersistence.swift | Grid layout save/restore |

## Views — Sidebar

| File | Purpose |
|---|---|
| SidebarView.swift | Sidebar tree view (projects → worktrees → agents) |
| AgentRow.swift | Agent row in sidebar |
| WorktreeRow.swift | Worktree row in sidebar |
| ProjectRow.swift | Project row in sidebar |
| SidebarFooter.swift | Sidebar footer (project picker, settings) |

## Views — Detail

| File | Purpose |
|---|---|
| DetailView.swift | Detail content area (terminal, dashboard) |

## Theme

| File | Purpose |
|---|---|
| PurePointTheme.swift | App-wide theme definitions |
| TerminalTheme.swift | Terminal color scheme and font settings |

## Total: 32 Swift files
