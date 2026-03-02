# Cross-Reference Matrix

Maps every product domain to its architecture dependencies, API operations, stored data, CLI commands, and desktop views. Use this to understand what a domain touches across the system.

| Domain | Architecture ADR | API Operations | Stored Data | CLI Commands | Desktop Views |
|---|---|---|---|---|---|
| Data Model | — | All (defines shared types) | worktrees, agents, events, memory | — | Models |
| CLI | — | All client-facing operations | — | All commands | — |
| Daemon | Daemon Engine, IPC & API | Health check, Init | daemon state | `pu health`, `pu init` | — |
| Agent Lifecycle | Agent Execution | Spawn, Kill, Restart, Status, Attach, Logs | agents, events | spawn, kill, restart, status, attach, logs | Agent list |
| Worktree Mgmt | — | Create, Merge, Diff, Clean | worktrees | worktree, merge, diff, clean | Sidebar |
| Orchestration | — | Swarm, Spawn | — | swarm, prompt, list | Swarms, Prompts |
| Scheduling | — | Schedule CRUD, Start/Stop | schedules | schedule commands | Schedules |
| Output & Streaming | Agent Execution | Logs, Stream Output, Attach | output chunks, summaries | logs, attach, aggregate | Terminal panes |
| Memory | — | Get/Set Memory | memory | — | — |
| Recovery | Daemon Engine | Recover | agents, worktrees | recover | — |
| Desktop App | Desktop App Integration | Watch Project, Stream Output | dashboard state, layout | ui | All views |
| Configuration | — | — | config | — | Settings |
