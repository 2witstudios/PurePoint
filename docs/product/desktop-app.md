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
    Point Guard (Pi chat, rich transcript, tools, conversation sidebar; separate root shell mode)
    Workspace files sidebar (inline changes review / filesystem previews)
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

! [APP-004] Point Guard defaults to native Pi chat using the shared Pi bridge v1 protocol. AppState retains PiChatModel across navigation; Pi owns native sessions, providers and tools. The transcript renders selectable prose, fenced code with copy, grouped expandable tool activity, notices and native extension dialogs. Conversation search, read-only browsing during a run, resume and new session use the bridge. Busy sends explicitly choose Steer or After reply; Stop clears the observed run's queue and aborts Pi without killing delegated workers. Drafts and uncertain receipts are persisted privately before transmission and never automatically replayed. Settings imports the bridge pairing secret from a local file into a desktop-specific Keychain service. Desktop and phone are simultaneous authenticated clients of one authoritative Pi session. Shared broadcasts reflect messages, tools, queues, dialogs and session changes on both devices. Mutations serialize centrally; stale session/run targets reject, and the first dialog answer wins. Stable per-device identities scope request IDs and canceled queue recovery. Disconnect removes only that view; other clients and Pi continue. Unsupported protocol versions and selection-label answers are rejected; this is the initial v1 contract. Shell mode preserves the existing daemon SpawnShell terminal, launch command, permissions and Claude/Codex/OpenCode history. Workspace split panes and ordinary terminal tabs retain their current behavior.

! [APP-005] Hotkey system: HotkeyMonitor registers OS-level shortcuts. KeyBindingState maps HotkeyAction → key equivalent. 31 actions across 5 categories (application: 3, navigation: 6, panes: 7, tabs: 14, chat: 1 — counted from `HotkeyAction.category`). Tabs: ⌘T new tab, ⌘W close tab (the `closeAgent` action; it closed the whole pane before tabs), ⇧⌘] / ⇧⌘[ next/previous, ⌘1–8 go to tab N, ⌘9 last tab, ⌥⌘D move tab to a new pane. ⌘1–9 are therefore taken; project switching (multi-project gap) should use ⌃⌘1–9. Customizable via Settings.

! [APP-007] Triggers: Event-driven automation via TriggersState. Supports agent_idle, pre_commit, pre_push events. Each trigger defines a sequence of actions (inject prompts, run gates). Managed via daemon IPC.

! [APP-010] Command palette order is a user preference in Settings → General. Up/down controls reorder built-in variants, visible agent definitions (including custom commands), and swarms together. Preferences persist in UserDefaults using palette item IDs, apply to all agent palettes, and retain positions for temporarily unavailable entries. Unranked entries follow in their default order. Reset restores the default order. Palettes load definitions before constructing the list so custom entries are available on the first open. All entry points share an open coordinator: retries during a pending load coalesce into one open, while invoking an already visible palette dismisses it immediately.

! [APP-006] NSOutlineView sidebar: Replaced SwiftUI List with AppKit NSOutlineView for compact 24pt rows. SidebarOutlineViewController manages outline data source.
! [APP-008] Files tab: a tab can show a file navigator + editor instead of a terminal. Offered in the empty-tab palette as "Files", and as per-file items once a query is typed. Rooted at the workspace worktree (else project root). Markdown files toggle Code/Preview (WKWebView, JS disabled, `MarkdownHTMLRenderer`). A file is a surface kind (`SurfaceContent.file(path:)`, see APP-009), so it sits in a pane's tab stack beside agent tabs; the version 2 `filePanes` side list is read only for migration.

! [APP-009] Pane tab stacks. Layout and content are separate: `PaneSplitNode` is geometry only (`leaf(id:)`, splits, ratios), and `Workspace.panes[leafId]` is a `Pane` — an ordered stack of `Surface`s with one active. A surface is `.agent(id)` (shells included), `.file(path?)` or `.empty` (palette pending), with an ID unique within the workspace that survives moves. Invariants (`Workspace.normalize`, `WorkspaceReconciler.reconcile`): every live agent occupies exactly one surface of one pane of one workspace; every pane has at least one tab; `panes` keys equal the tree's leaf IDs. Operations are pure `Workspace` methods: `newTab` (after the active tab), `selectTab`/`cycleTab`, `closeTab` (a pane's last tab closes the pane; the workspace's last tab removes the workspace), `moveTab` (between or within panes, or before a given tab; the agent keeps running), and `breakTab` (tab → new pane beside it, like tmux break-pane; moving the tab onto another pane's strip reverses it). A workspace survives as long as it holds an agent tab or a file tab, or a spawn is pending into one of its empty tabs (`Workspace.isGhost`, applied by both the reconciler and tab closing). Closing tabs therefore never strands a file tab, and closing the sidebar row (`closeWorkspace`) removes everything in it. Only the active tab renders. Background terminals stay attached in `TerminalViewCache`, which flags live output that arrives while they are off screen (`unseenOutput`, the tab's dot, cleared when shown). Neither the daemon's buffer replay nor the redraw that follows a resize counts as live output. A file tab's navigator and editor state, unsaved edits included, live in `FileTabStore`, keyed by workspace and tab, so they survive the tab being hidden or moved. Closing a tab with unsaved edits asks first, and the tab's title and saved path follow the file shown. The tab strip is always visible and carries the pane actions (new tab, split right/below, close pane — the last confirms when it would stop more than one running agent). Tabs drag to reorder or onto another pane's strip to move. Persisted in `.pu/workspaces.json` version 3: each leaf node carries `tabs` (`{id, kind: agent|file|empty, agentId?, path?}`) and `activeTabId`, each workspace `nextSurfaceId`. The document also records `activeWorkspaceId` for the CLI. Version 2 files migrate on load: a leaf's `agentId`, else its `filePanes` entry, else an empty tab, becomes its single tab. A file written by a newer version is never overwritten. Remote control: `pu grid tab new|select|close|move|break` (`GridCommand` `new_tab`, `select_tab`, `close_tab`, `move_tab`, `break_tab`). Tab positions are 1-based and tab IDs are surface IDs. Every grid command may name a `workspace_id`, because leaf and tab IDs are only unique within a workspace; otherwise it applies to the workspace on screen, which `pu grid show` marks `(active)`.

! [APP-010] Workspace files sidebar. A workspace's pane grid has a resizable, optional right sidebar at the same visual level as its panes, with a plain divider and no elevated material. Changes is the default mode: compact filename/count rows expand independently into selectable, syntax-highlighted unified diffs directly beneath the row, within one scroll view. Review compares HEAD to the working tree, including staged edits, unstaged edits, deleted and untracked files; repositories without a first commit show additions. Diff bodies load on expansion. Files mode shows a lazy directory tree with read-only file previews expanding in place. Neither mode changes the active terminal tab or pane focus. The root follows the workspace's worktree, falling back to its project root (same routing as file tabs). Sidebar visibility is persisted in UserDefaults; mode and expansion survive hiding it, and reset when the root changes. While visible, summaries and expanded content refresh every two seconds, including ordinary file saves that do not change `.git`. Previews are bounded to 1 MB, with explicit empty, binary, oversized and read-error states. Owner-approved design: [inline review canvas](https://pagespace.ai/dashboard/nmyzsuuo4kc1u4y2e9vsndft/ow4awdtfjvas5dd9cx6tg2sf).
