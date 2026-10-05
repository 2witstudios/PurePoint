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

Split your workspace into panes:

| Action | Shortcut |
|---|---|
| Split Right | `Shift+Cmd+D` |
| Split Below | `Cmd+D` |
| Close Pane | `Shift+Cmd+W` |
| Focus Up / Down / Left / Right | `Option+Cmd+Arrow` |

Drag dividers to resize. Layouts persist across sessions.

## Tabs

Every pane holds a stack of tabs — agents, shells, or files — shown in a strip along its top. Background tabs keep running; a blue dot marks a tab whose terminal printed output since you last looked at it.

| Action | Shortcut |
|---|---|
| New Tab (opens the palette) | `Cmd+T` |
| Close Tab | `Cmd+W` |
| Next / Previous Tab | `Shift+Cmd+]` / `Shift+Cmd+[` |
| Go to Tab 1–8 | `Cmd+1` … `Cmd+8` |
| Go to Last Tab | `Cmd+9` |
| Move Tab to New Pane | `Option+Cmd+D` |

Drag a tab to reorder it, or drop it on another pane's strip to move it there. Closing a pane's last tab closes the pane. All shortcuts can be changed in Settings → Hotkeys.

From the CLI, `pu grid show` lists each pane's tabs, `pu grid tab new --agent <id>` opens an agent in a new tab, and `pu grid assign <agent_id>` puts an agent in the focused pane's active tab.

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

- **General**: Restore projects on launch, launch at login
- **Point Guard**: Configure launch command and skip-permissions for the root terminal
- **Agents**: Per-agent configuration (launch arguments for Claude, Codex, OpenCode)
- **Hotkeys**: Rebind all keyboard shortcuts with live key recording and conflict detection
- **Display**: Appearance preferences, terminal font size, grid gap
- **About**: Version and updates (auto-update via Sparkle)

## Triggers

Automate workflows based on events:

- **Agent Idle**: Run actions when an agent becomes idle
- **Pre-Commit**: Gate commits with validation scripts
- **Pre-Push**: Run checks before pushing

Each trigger can inject prompts, run shell commands with exit code gates, and use variables. Configure via the daemon IPC or CLI.

## Keyboard shortcuts

All actions are rebindable in Settings > Hotkeys.
