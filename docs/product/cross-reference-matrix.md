# Cross-Reference Matrix

Maps every product domain to its architecture dependencies, API operations, stored data, CLI commands, and desktop views. Use this to understand what a domain touches across the system.

| Domain | Architecture ADR | API Operations | Stored Data | CLI Commands | Desktop Views |
|---|---|---|---|---|---|
| Data Model | — | All (defines shared types) | worktrees, agents, events, memory | — | Models |
| CLI | — | All client-facing operations | — | All commands | — |
| Daemon | Daemon Engine, IPC & API | Health check, Init | daemon state | `pu health`, `pu init` | — |
| Agent Lifecycle | Agent Execution | Spawn, Kill, Restart, Status, Attach, Logs | agents, events | spawn, kill, restart, status, attach, logs | Agent list, Chat/PointGuard, ConversationSidebar |
| Worktree Mgmt | — | Create, Merge, Diff, Clean, Rename, Delete | worktrees | worktree, merge, diff, clean | Sidebar, WorktreeDetailView, DiffViewer |
| Orchestration | — | Swarm, Spawn, Template CRUD, AgentDef CRUD, SwarmDef CRUD | templates, agent_defs, swarm_defs | prompt, agent, swarm | AgentsHub (prompts, agent defs, swarms), creation sheets |
| Scheduling | — | Schedule CRUD, Enable/Disable | schedules | schedule list/show/create/delete/enable/disable | ScheduleView, calendar views, ScheduleCreationSheet |
| Output & Streaming | Agent Execution | Logs, Stream Output, Attach | output chunks, summaries | logs, attach, aggregate | Terminal panes, ChatAreaView |
| Memory | — | Get/Set Memory | memory | — | — |
| Recovery | Daemon Engine | Recover | agents, worktrees | recover | — |
| Desktop App | Desktop App Integration | Watch Project, Stream Output | dashboard state, layout | ui | All views, CommandPalette, Settings, HotkeySystem |
| Configuration | — | — | config | — | SettingsView (general, display, hotkeys, about) |
