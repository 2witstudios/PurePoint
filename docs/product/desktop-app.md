# Desktop App

**Maturity: SEED** | ID Prefix: APP | Dependencies: `architecture/desktop-app-integration.md`

## Purpose

PurePoint.app — the macOS desktop application. Primary visual interface for managing agents, viewing output, editing configurations, and monitoring project state. Swift/AppKit, talks to daemon via gRPC.

## Open Questions

? [APP-001] Should the desktop app support multiple simultaneous project connections, or one project at a time?

? [APP-002] How should the app handle daemon disconnection — auto-reconnect, show an error overlay, or gracefully degrade to read-only?

## Conceptual Model

```
PurePoint.app
  Daemon connection (gRPC client)
  Sidebar (project tree: worktrees → agents)
    Subscribes to WatchProject for real-time tree updates
  Content area (tab-based views)
    Home Dashboard (commit heatmap, agent status cards)
      Subscribes to WatchProject stream (replaces manifest polling)
    Terminal views (SwiftTerm)
      Consume StreamOutput gRPC stream (replaces local process spawning)
    Pane Grid (recursive binary split, up to 6 panes)
      Split (horizontal | vertical)
        TerminalPane (connected to agent via StreamOutput)
        Split → TerminalPane...
    Editors (swarms, prompts, schedules, config)
  Command Palette
  Settings
```
