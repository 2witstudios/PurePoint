# Desktop App

**Maturity: CONVERGING** | ID Prefix: APP | Dependencies: `architecture/desktop-app-integration.md`

## Purpose

The desktop application — primary visual interface for managing agents, viewing output, and monitoring project state. Reads workspace state from the daemon via IPC and connects to agent terminals via the daemon's attach protocol.

## Conceptual Model

```
Desktop App
  App lifecycle (CLIInstaller, DaemonLifecycle, agent restoration)
  Sidebar (NSOutlineView: projects → worktrees → agents)
    Live updates via manifest watcher + daemon status
    Context menus (rename, delete worktree, kill all)
  Content area
    Terminal views (SwiftTerm → daemon attach/output)
    Terminal view cache (hide/show, LRU eviction)
    Pane Grid (binary tree splits, spatial nav, daemon sync)
    Point Guard (root terminal, configurable launch command, conversation sidebar)
    Project + Worktree detail views (inline diff viewer)
  Conversation sidebar (session search, timeline grouping)
  Agents Hub (prompt library, agent definitions, swarm definitions)
  Schedule (calendar views, CRUD via daemon)
  Triggers (event-driven automation: agent_idle, pre_commit, pre_push)
  Settings (general, point guard, agents, hotkeys, display, about)
  Command palette (agent spawning — built-in variants, agent defs, swarms)
  Hotkey system (17 OS-level shortcuts across 4 categories, customizable)
```

## Decisions

! [APP-001] Multi-project: AppState holds array of ProjectState, one per git root. Cross-project queries via `agent(byId:)`. Project persistence via UserDefaults. Unified state with per-project instances.

! [APP-002] Daemon required. Auto-started on project open via `DaemonLifecycle` with `--managed` flag. On app quit, sends `Request::Shutdown` to stop the daemon and all agents. Graceful degradation when daemon is unreachable — sidebar shows empty state, project picker available.

! [APP-003] macOS only. Native SwiftUI + AppKit bridges.

! [APP-004] Point Guard: PointGuardView spawns a shell via daemon's SpawnShell request and auto-launches the configured agent. SessionListState manages the conversation sidebar. ConversationSidebarView with search and timeline grouping. Configurable launch command and skip-permissions via SettingsState (SettingsPointGuardView).

! [APP-005] Hotkey system: HotkeyMonitor registers OS-level shortcuts. KeyBindingState maps HotkeyAction → key equivalent. 17 actions across 4 categories (application: 4, navigation: 7, panes: 7, chat: 1). Customizable via Settings.

! [APP-007] Triggers: Event-driven automation via TriggersState. Supports agent_idle, pre_commit, pre_push events. Each trigger defines a sequence of actions (inject prompts, run gates). Managed via daemon IPC.

! [APP-006] NSOutlineView sidebar: Replaced SwiftUI List with AppKit NSOutlineView for compact 24pt rows. SidebarOutlineViewController manages outline data source.
