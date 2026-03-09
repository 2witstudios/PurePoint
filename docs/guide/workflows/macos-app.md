# macOS App

PurePoint's macOS app provides a graphical workspace for managing agents.

## Opening a project

Launch PurePoint from Applications. Add a project directory. The app reads `.pu/manifest.json` to discover existing worktrees and agents.

Multiple projects can be open simultaneously, each with its own agents and worktrees.

## Sidebar

The sidebar shows all worktrees and agents with live status indicators:

- Green: streaming (actively working)
- Yellow: waiting (idle)
- Red: broken (exited)

Click any agent to view its terminal in the main area.

## Command palette

Open with `Cmd+N`. Gives instant access to:

- Built-in agent types (Claude, Codex, OpenCode, Terminal)
- Your custom agent defs
- Saved swarms

Pick one, name the worktree, enter a prompt, and spawn. Fuzzy search narrows the list.

## Pane grid

Split your workspace into terminal panes:

| Action | Shortcut |
|---|---|
| Split Right | `Cmd+K Cmd+L` |
| Split Below | `Cmd+K Cmd+J` |
| Close Pane | `Cmd+K Q` |
| Focus Up | `Cmd+K Up` |
| Focus Down | `Cmd+K Down` |
| Focus Left | `Cmd+K Left` |
| Focus Right | `Cmd+K Right` |

Drag dividers to resize. Layouts persist across sessions.

Each pane displays one agent's terminal. Use `pu grid assign <agent_id>` from the CLI to assign agents to panes.

## Point Guard

Point Guard is where you direct the work. It's a terminal that auto-launches your configured coding agent (Claude Code by default). From here you can:

- Spawn agents and delegate tasks
- Start new projects
- Direct and coordinate ongoing work

Your conversation history lives in the sidebar — search past sessions and resume where you left off. Configure the launch command and permissions in Settings. Supports Claude, Codex, and OpenCode.

## Diff viewer

Review agent work without leaving PurePoint:

- Unstaged changes per worktree
- PR diffs via `gh` CLI integration
- Syntax-highlighted inline diffs
- Modified files list with change indicators

## Agents Hub

Three tabs for managing definitions:

- **Prompts**: Create, edit, and browse prompt templates
- **Agents**: Named agent defs with type, template, and tags
- **Swarms**: Multi-agent compositions with roster and execution config

## Schedule calendar

Browse schedules in month, week, day, or list view. See upcoming scheduled runs and their configurations.

## Settings

Access via the settings panel:

- **General**: Default behaviors
- **Display**: Appearance preferences
- **Hotkeys**: Rebind all keyboard shortcuts with live key recording and conflict detection
- **About**: Version and updates (auto-update via Sparkle)

## Keyboard shortcuts

All actions are rebindable in Settings > Hotkeys.
