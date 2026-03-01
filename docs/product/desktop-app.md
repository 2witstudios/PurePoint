# Desktop App

**Maturity: DECIDED** | ID Prefix: APP | Dependencies: `architecture/desktop-app-integration.md`

## Purpose

The desktop application — primary visual interface for managing agents, viewing output, and monitoring project state. Reads workspace state from `.pu/manifest.json` and connects to agent terminals via tmux grouped sessions.

## Conceptual Model

```
Desktop App
  Manifest file watcher (.pu/manifest.json)
  Sidebar (project tree: worktrees → agents)
    Live updates from manifest file changes
  Content area
    Terminal views (SwiftTerm → tmux grouped session)
    Terminal view cache (hide/show, LRU eviction)
    Pane Grid (split layout, up to 6 terminals)
    Dashboard (agent status summary)
  Project picker (folder browser, recent projects)
```

## Decisions

! [APP-001] One project at a time initially. Recent projects stored in UserDefaults for quick switching.

! [APP-002] No daemon. Graceful degradation when manifest doesn't exist — sidebar shows empty state, project picker available.

! [APP-003] macOS only. Native SwiftUI + AppKit bridges.
