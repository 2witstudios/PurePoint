# Cross-Reference Matrix

Maps every product domain to its architecture dependencies, API operations, stored data, CLI commands, and desktop views. Use this to understand what a domain touches across the system.

| Domain | Architecture ADR | API Operations | Stored Data | CLI Commands | Desktop Views |
|---|---|---|---|---|---|
| Data Model | — | All (defines shared types) | worktrees, agents, events, memory | — | Models |
| CLI | — | All client-facing operations | — | All commands | — |
| Daemon | Daemon Engine, IPC & API | Health check, Init | daemon state | `pu health`, `pu init` | — |
| Agent Lifecycle | Agent Execution | Spawn, Kill, Restart, Status, Attach, Logs | agents, events | spawn, kill, restart, status, attach, logs | Agent list, Chat/PointGuard, ConversationSidebar |
| Worktree Mgmt | — | Create, Merge, Diff, Clean, Rename, Delete | worktrees | worktree, merge, diff, clean | Sidebar, WorktreeDetailView, DiffViewer |
| Orchestration | — | Swarm, Spawn, Template CRUD, AgentDef CRUD, SwarmDef CRUD | templates, agent_defs, swarm_defs | prompt, agent, swarm | AgentsHub (prompts, agent defs, swarms), creation sheets, Chat/PointGuard court plays (global prompts) |
| Scheduling | — | Schedule CRUD, Enable/Disable | schedules | schedule list/show/create/delete/enable/disable | ScheduleView, calendar views, ScheduleEventSheet |
| Output & Streaming | Agent Execution | Logs, Stream Output, Attach | output chunks, summaries | logs, attach, aggregate | Terminal panes, ChatAreaView |
| Memory | — | Get/Set Memory | memory | — | — |
| Recovery | Daemon Engine | Recover | agents, worktrees | recover | — |
| Desktop App | Desktop App Integration | Watch Project, Stream Output | dashboard state, layout | ui | All views, CommandPalette, Settings, HotkeySystem |
| Configuration | — | — | config | — | SettingsView (general, display, hotkeys, about) |
| Machines | IPC & API, Daemon Engine, Desktop App Integration | Health (version check), FsList, FsStat, FsRead, FsWrite, FsDelete, SubscribeFs, GitExec, plus every project/agent operation over the forwarded socket | `~/.pu/machines.json`, `~/.pu/machines/<id>/daemon.sock` | `--machine <name>`, machine list/add/remove/status/start | Sidebar machine sections, Settings (machines) |
| Linux Phone Host | Distribution, Desktop App Integration | Point Guard admin (status, auth.*, pairing.*, devices.*), daemon ops via the bridge | `${XDG_STATE_HOME}/purepoint/point-guard/`, `~/.pi/agent/`, `~/.pu/pointguard/<version>/` | `pu phone` install/uninstall/start/stop/restart/status/run/pair/devices/grant/rename/revoke/login/model/cwd | iOS Connections (host kind) |
| iPad App | Desktop App Integration, IPC & API (via Machines, Desktop App, Point Guard Setup) | Every Desktop App and Machines operation, over SSH exec channels running `pu daemon proxy`; Health for machine state and Start daemon; Point Guard bridge v1 over pinned WSS | iPad container `Application Support/PurePoint/machines.json`; Keychain `PurePoint.SSHIdentity.v1`, `PurePoint.HostKeyPin.v1`, `PiMobile.TrustedHost.v1` | `pu daemon proxy` (run remotely over SSH), `pu health` (Start daemon) | All Desktop App views via `purepoint-ipad` (UIKit twins), Add Machine (device key, host-key trust), remote folder picker, Point Guard pairing (QR, saved Macs) |
| Windows App | Daemon Engine, IPC & API, Distribution, Desktop App Integration (via Machines, Desktop App, iPad App) | Every Desktop App and Machines operation over the private named pipe `\\.\pipe\purepoint-<hash16>-daemon` (local) or `ssh.exe <host> pu daemon proxy` (remote); Health for daemon lifecycle and machine state; protocol 14 unchanged | `%USERPROFILE%\.pu\` (`daemon.sock.{pid,lock,projects.json}`, `daemon.log`, `machines.json`, `bin\pu.exe`, `bin\pu-engine.exe`); HKCU `Run` value `PurePoint Engine`; Tauri app data (windows, selection, preferences, key bindings); Credential Manager `PurePoint.PointGuard.Pairing` | Every command on Windows; `pu daemon install/uninstall/status` (Run value), `pu daemon proxy`, `--machine` via `ssh.exe` | All Desktop App views in `apps/purepoint-windows` (Tauri 2 + React + xterm.js), custom title bar, Tauri folder picker, toasts and taskbar badge, updater |
