# Swift App Inventory

Source map of the macOS desktop app (`apps/purepoint-macos/purepoint-macos/`).

## App Lifecycle

| File | Purpose |
|---|---|
| purepoint_macosApp.swift | SwiftUI app entry point — creates AppState + TerminalViewCache, injects via .environment() |
| ContentView.swift | Root content view (sidebar + detail layout) |

## Models

| File | Purpose |
|---|---|
| AgentStatus.swift | 3-state enum (Streaming, Waiting, Broken) — normalizes legacy values, derives nsColor/isAlive from `normalized` |
| AgentVariant.swift | Struct with Kind enum (.agent, .terminal, .worktree) — static properties for built-in variants (Claude, Codex, etc.) with icon/subtitle |
| AgentsHubModels.swift | SavedPrompt, AgentDefinition, SwarmDefinition — hub library types |
| ChatMessage.swift | ChatMessage, ContentBlock, ToolUseStatus, PulseEvent, PulseSummary |
| ClaudeConversation.swift | Session metadata (sessionId, title, projectPath, gitBranch, timestamps) |
| CommandPaletteItem.swift | Enum: .builtIn(AgentVariant), .agentDef(AgentDefinition), .swarm(SwarmDefinition) — palette items with displayName/icon |
| ContentBlockSplitter.swift | Parse streamed content into code blocks, text, tool calls |
| DiffModel.swift | Git diff representation |
| KeyBinding.swift | HotkeyAction enum (20+ actions), HotkeyCategory |
| ManifestModel.swift | Manifest JSON decoding (mirrors pu-core Rust types) |
| PRModel.swift | Pull request data |
| ScheduleEvent.swift | Calendar event structure |
| SidebarItem.swift | SidebarNavItem enum (.dashboard, .agents, .schedule) + SidebarSelection enum (.nav, .agent, .terminal, .worktree, .project) |
| StreamEvent.swift | Daemon stream events (assistant, contentBlockDelta, toolResult, result, error) |
| WorkspaceModel.swift | Workspace/agent view models (WorkspaceModel, AgentModel) |

## State

| File | Purpose |
|---|---|
| AppState.swift | @Observable @MainActor — multi-project container with projects array, selectedAgentId, activeProjectRoot, sidebar selection, daemon error |
| AgentsHubState.swift | Templates, agent defs, swarm defs, selection state |
| ChatState.swift | Chat UI: messages, sessions, streaming, input text, search query, conversation loading |
| DiffState.swift | Diff viewing state |
| GridState.swift | @Observable — pane grid layout: root node, focusedLeafId, ownerAgentId, pendingPaletteLeafId, onCloseAgent callback |
| KeyBindingState.swift | Hotkey-to-key mappings, delegates to HotkeyMonitor |
| ProjectState.swift | @Observable @MainActor — per-project: rootAgents, worktrees, manifest watcher, weak refs to gridState/appState |
| ScheduleState.swift | Schedule events, loading/error state |
| SettingsState.swift | User preferences: appearance, font sizes |

## Services

| File | Purpose |
|---|---|
| CLIInstaller.swift | Copy pu binary + skill from app bundle to ~/.pu/bin on launch (mod-date freshness check) |
| ClaudeConversationIndex.swift | Two-phase session loading (fast index + slow JSONL scan) |
| ClaudeProcess.swift | Spawn claude CLI process for streaming conversations |
| DaemonAttachSession.swift | Streaming attach session for live PTY output |
| DaemonClient.swift | NDJSON-over-Unix-socket client for daemon IPC |
| DaemonGridSubscription.swift | Subscribe to grid layout updates from daemon |
| DaemonLifecycle.swift | Daemon auto-start, health check, graceful shutdown |
| DaemonWorkspaceService.swift | WorkspaceService implementation backed by daemon IPC |
| GitService.swift | Git operations (branch, status, PR diff, cached gh binary path) |
| HotkeyMonitor.swift | OS-level hotkey registration via NotificationCenter |
| ManifestWatcher.swift | DispatchSource file watcher on .pu/manifest.json (triggers daemon refresh) |
| NSView+Constraints.swift | Layout constraint helpers |
| ShellUtilities.swift | Shell command execution utilities |
| TranscriptParser.swift | Parse JSONL transcripts into ChatMessage arrays |
| WorkspaceService.swift | Protocol defining workspace operations |
| WorktreeWatcher.swift | Worktree change detection |

## Views — Terminal

| File | Purpose |
|---|---|
| ScrollableTerminal.swift | Scrollable terminal container |
| TerminalContainerView.swift | Terminal container with toolbar and status |
| TerminalPaneView.swift | SwiftTerm terminal pane (NSViewRepresentable) |
| TerminalViewCache.swift | Terminal view cache (hide/show, LRU eviction after 30s) |

## Views — Pane Grid

| File | Purpose |
|---|---|
| DraggableSplit.swift | Draggable split handle for pane resizing |
| GridLayoutPersistence.swift | Grid layout save/restore |
| PaneCellView.swift | Individual pane cell in grid |
| PaneGridView.swift | Pane grid system (split layout) |
| PaneSplitNode.swift | Recursive binary split node (indirect enum) |

## Views — Sidebar

| File | Purpose |
|---|---|
| SidebarFooter.swift | Settings (gear) + command palette (plus icon) buttons |
| SidebarOutlineView.swift | NSViewControllerRepresentable wrapping SidebarOutlineViewController |
| SidebarOutlineViewController.swift | NSViewController for AppKit NSOutlineView — compact 24pt rows |
| SidebarView.swift | Sidebar container — wraps NSOutlineView (projects → worktrees → agents) |

## Views — Detail

| File | Purpose |
|---|---|
| DetailView.swift | Detail content area (terminal, dashboard, project/worktree detail) |
| DiffCardView.swift | Inline diff card display |
| DiffContentNSView.swift | AppKit NSView for diff content rendering |
| DiffListView.swift | List of diffs |
| PRRowView.swift | Pull request row display |
| ProjectDetailView.swift | Project detail view |
| WorktreeDetailView.swift | Worktree detail view with diff viewer |

## Views — Chat

| File | Purpose |
|---|---|
| ChatAreaView.swift | Main chat display area |
| ChatInputView.swift | Message input with send |
| CodeBlockView.swift | Syntax-highlighted code block |
| ContentBlockView.swift | Routes to text/code/tool blocks |
| ConversationSidebarView.swift | Session list with search and timeline grouping |
| MarkdownTextView.swift | Markdown rendering |
| MessageBubbleView.swift | User/assistant message bubble |
| PointGuardView.swift | Token/point guard UI |
| ToolCallCardView.swift | Tool use display card |

## Views — Agents Hub

| File | Purpose |
|---|---|
| AgentCreationSheet.swift | Create agent dialog |
| AgentsHubView.swift | Prompts, agent defs, swarms library |
| PromptCreationSheet.swift | Create prompt dialog |
| SwarmCreationSheet.swift | Create swarm dialog |

## Views — Settings

| File | Purpose |
|---|---|
| SettingsAboutView.swift | App version, build, logo |
| SettingsDisplayView.swift | Appearance, font sizes |
| SettingsGeneralView.swift | General preferences |
| SettingsHotkeysView.swift | Hotkey customization |
| SettingsSection.swift | Reusable settings section component |
| SettingsView.swift | Modal settings panel |

## Views — Schedule

| File | Purpose |
|---|---|
| DayCalendarView.swift | Day calendar view |
| EventBlockView.swift | Event block display |
| EventPillView.swift | Compact event pill display |
| MonthCalendarView.swift | Month calendar view |
| ScheduleCreationSheet.swift | Create event sheet |
| ScheduleHeaderView.swift | Schedule header with navigation |
| ScheduleListView.swift | Event list view |
| ScheduleView.swift | Calendar + time grid container |
| TimeGridView.swift | Time-based event grid |
| WeekCalendarView.swift | Week calendar view |

## Views — Command Palette

| File | Purpose |
|---|---|
| CommandPalettePanel.swift | NSPanel (floating, borderless) — agent spawning palette |

## Views — Other

| File | Purpose |
|---|---|
| DaemonErrorBanner.swift | Error display overlay |
| MockWorkspaceComponents.swift | Mock surface card component for UI prototyping |

## Theme

| File | Purpose |
|---|---|
| PurePointTheme.swift | App-wide theme definitions |
| TerminalTheme.swift | Terminal color scheme and font settings |
| Theme.swift | Additional theme data |

## Total: 97 Swift files
