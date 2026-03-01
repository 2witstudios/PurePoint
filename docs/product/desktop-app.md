# Desktop App

**Maturity: SEED** | ID Prefix: APP | Dependencies: `architecture/desktop-app-integration.md`

## Purpose

The desktop application — primary visual interface for managing agents, viewing output, editing configurations, and monitoring project state. A client to the daemon.

## Conceptual Model

```
Desktop App
  Daemon connection (API client)
  Sidebar (project tree: worktrees → agents)
    Real-time tree updates from daemon
  Content area (tab-based views)
    Dashboard (activity overview, agent status)
    Terminal views (live agent output)
    Pane Grid (multiple terminals in split layout)
    Editors (swarms, prompts, schedules, config)
  Command Palette
  Settings
```

## Open Questions

? [APP-001] Should the desktop app support multiple simultaneous project connections, or one project at a time?

? [APP-002] How should the app handle daemon disconnection — auto-reconnect, show an error overlay, or gracefully degrade to read-only?

? [APP-003] What platform(s) should the desktop app target initially?
