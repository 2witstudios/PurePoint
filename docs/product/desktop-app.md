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
    Chat / Point Guard (Claude conversation, streaming, history)
    Project + Worktree detail views (inline diff viewer)
  Conversation sidebar (session search, timeline grouping)
  Agents Hub (prompt library, agent definitions, swarm definitions)
  Schedule (calendar views, CRUD via daemon)
  Settings (appearance, hotkeys, about)
  Command palette (agent spawning — built-in variants, agent defs, swarms)
  Hotkey system (20+ OS-level shortcuts, customizable)
```

## Decisions

! [APP-001] Multi-project: AppState holds array of ProjectState, one per git root. Cross-project queries via `agent(byId:)`. Project persistence via UserDefaults. Unified state with per-project instances.

! [APP-002] Daemon required. Auto-started on project open via `DaemonLifecycle` with `--managed` flag. On app quit, sends `Request::Shutdown` to stop the daemon and all agents. Graceful degradation when daemon is unreachable — sidebar shows empty state, project picker available.

! [APP-003] macOS only. Native SwiftUI + AppKit bridges.

! [APP-004] Chat interface: ChatState manages Claude conversation streaming via ClaudeProcess. ClaudeConversationIndex provides two-phase session loading (fast index → slow JSONL scan). Conversation sidebar with search and timeline grouping.

! [APP-005] Hotkey system: HotkeyMonitor registers OS-level shortcuts. KeyBindingState maps HotkeyAction → key equivalent. 20+ actions across 4 categories (application, navigation, panes, chat). Customizable via Settings.

! [APP-006] NSOutlineView sidebar: Replaced SwiftUI List with AppKit NSOutlineView for compact 24pt rows. SidebarOutlineViewController manages outline data source.
