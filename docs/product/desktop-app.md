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
      Pane tab stacks (agent / file / empty surfaces per pane)
    Point Guard (root terminal, configurable launch command, conversation sidebar)
    Project + Worktree detail views (inline diff viewer)
  Conversation sidebar (session search, timeline grouping)
  Agents Hub (prompt library, agent definitions, swarm definitions)
  Schedule (calendar views, CRUD via daemon)
  Triggers (event-driven automation: agent_idle, pre_commit, pre_push)
  Settings (general, point guard, agents, hotkeys, display, about)
  Command palette (agent spawning — built-in variants, agent defs, swarms)
  Hotkey system (31 shortcuts across 5 categories, customizable)
```

## Decisions

! [APP-001] Multi-project: AppState holds array of ProjectState, one per git root. Cross-project queries via `agent(byId:)`. Project persistence via UserDefaults. Unified state with per-project instances.

! [APP-002] Daemon required. Auto-started on project open via `DaemonLifecycle` with `--managed` flag. On app quit, sends `Request::Shutdown` to stop the daemon and all agents. Graceful degradation when daemon is unreachable — sidebar shows empty state, project picker available.

! [APP-003] macOS only. Native SwiftUI + AppKit bridges.

! [APP-004] Point Guard: PointGuardView spawns a shell via daemon's SpawnShell request and auto-launches the configured agent. SessionListState manages the conversation sidebar. ConversationSidebarView with search and timeline grouping. Configurable launch command and skip-permissions via SettingsState (SettingsPointGuardView).

! [APP-005] Hotkey system: HotkeyMonitor registers OS-level shortcuts. KeyBindingState maps HotkeyAction → key equivalent. 31 actions across 5 categories (application: 3, navigation: 6, panes: 7, tabs: 14, chat: 1 — counted from `HotkeyAction.category`). Tabs: ⌘T new tab, ⌘W close tab (the `closeAgent` action; it closed the whole pane before tabs), ⇧⌘] / ⇧⌘[ next/previous, ⌘1–8 go to tab N, ⌘9 last tab, ⌥⌘D move tab to a new pane. ⌘1–9 are therefore taken; project switching (multi-project gap) should use ⌃⌘1–9. Customizable via Settings.

! [APP-007] Triggers: Event-driven automation via TriggersState. Supports agent_idle, pre_commit, pre_push events. Each trigger defines a sequence of actions (inject prompts, run gates). Managed via daemon IPC.

! [APP-006] NSOutlineView sidebar: Replaced SwiftUI List with AppKit NSOutlineView for compact 24pt rows. SidebarOutlineViewController manages outline data source.
! [APP-008] Files tab: a tab can show a file navigator + editor instead of a terminal. Offered in the empty-tab palette as "Files", and as per-file items once a query is typed. Rooted at the workspace worktree (else project root). Markdown files toggle Code/Preview (WKWebView, JS disabled, `MarkdownHTMLRenderer`). A file is a surface kind (`SurfaceContent.file(path:)`, see APP-009), so it sits in a pane's tab stack beside agent tabs; the version 2 `filePanes` side list is read only for migration.

! [APP-009] Pane tab stacks. Layout and content are separate: `PaneSplitNode` is geometry only (`leaf(id:)`, splits, ratios), and `Workspace.panes[leafId]` is a `Pane` — an ordered stack of `Surface`s with one active. A surface is `.agent(id)` (shells included), `.file(path?)` or `.empty` (palette pending), with an ID unique within the workspace that survives moves. Invariants (`Workspace.normalize`, `WorkspaceReconciler.reconcile`): every live agent occupies exactly one surface of one pane of one workspace; every pane has at least one tab; `panes` keys equal the tree's leaf IDs. Operations are pure `Workspace` methods: `newTab` (after the active tab), `selectTab`/`cycleTab`, `closeTab` (a pane's last tab closes the pane; the workspace's last tab removes the workspace), `moveTab` (between or within panes; agent keeps running), `breakTab` (tab → new pane beside it, like tmux break-pane), `joinPane` (inverse). Only the active tab renders; background terminals stay attached in `TerminalViewCache`, which flags output that arrives while off screen (`unseenOutput`, the tab's dot, cleared when shown). The tab strip is always visible and carries the pane actions (new tab, split right/below, close pane — the last confirms when it would stop more than one running agent). Tabs drag to reorder or onto another pane's strip to move. Persisted in `.pu/workspaces.json` version 3: each leaf node carries `tabs` (`{id, kind: agent|file|empty, agentId?, path?}`) and `activeTabId`, each workspace `nextSurfaceId`. Version 2 files migrate on load (a leaf's `agentId`, else its `filePanes` entry, else empty, becomes its single tab). Remote control: `pu grid tab new|select|close|move|break` (`GridCommand` `new_tab`, `select_tab`, `close_tab`, `move_tab`, `break_tab`; tab positions 1-based, tab IDs are surface IDs), applied to the active workspace like the other grid commands.
