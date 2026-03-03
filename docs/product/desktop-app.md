# Desktop App

**Maturity: DECIDED** | ID Prefix: APP | Dependencies: `architecture/desktop-app-integration.md`

## Purpose

The desktop application — primary visual interface for managing agents, viewing output, and monitoring project state. Reads workspace state from the daemon via IPC and connects to agent terminals via the daemon's attach protocol.

## Conceptual Model

```
Desktop App
  Manifest file watcher (triggers daemon refresh)
  Sidebar (project tree: worktrees → agents)
    Live updates via daemon status queries
  Content area
    Terminal views (SwiftTerm → daemon attach/output)
    Terminal view cache (hide/show, LRU eviction)
    Pane Grid (split layout, up to 6 terminals)
    Dashboard (agent status summary)
  Project picker (folder browser, recent projects)
```

## Decisions

? [APP-001] How should multi-project support work?
Current implementation opens one project at a time with recent projects in UserDefaults for quick switching, but this was not a deliberate design constraint. Options: tabs (like ppg-cli), separate windows (more macOS-native), or unified sidebar with project grouping. Daemon already supports multi-project via `project_root` parameter.

! [APP-002] Daemon required. Auto-started on project open via `DaemonLifecycle` with `--managed` flag. On app quit, sends `Request::Shutdown` to stop the daemon and all agents. Graceful degradation when daemon is unreachable — sidebar shows empty state, project picker available.

! [APP-003] macOS only. Native SwiftUI + AppKit bridges.
