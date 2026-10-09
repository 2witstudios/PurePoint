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

Point Guard opens a native Pi chat with selectable rich responses, copyable code and expandable tool activity. Search and resume native Pi conversations from the sidebar. Keep typing while Pi works; use **Send after reply** or **Steer current work** for busy sends. **Stop** cancels the Pi run and queued messages while delegated PurePoint workers continue. Uncertain or canceled sends have explicit Restore/Dismiss actions and are never resent automatically.

Start the [Pi bridge](../../../apps/purepoint-mobile/README.md#mac-setup), then open **Settings → Point Guard**. Enter its address ending in `/v1` (use the Tailscale address if that is where it listens) and the pairing secret file, normally `~/.config/pi-mobile/pairing-secret`. Connect imports the secret into desktop Keychain. Desktop and phone can stay connected simultaneously: both see the same live conversation, tools, queue and session changes, and both can send or Stop. The bridge serializes actions; the first dialog answer wins. Canceled queued text returns to its originating device. Disconnecting one client leaves Pi and the other clients running. Both apps and the bridge use the initial v1 contract at `/v1`.

Pi is a global assistant: name the project when directing work. The current workspace is shown as context; selecting it does not silently change Pi's working folder.

Choose **Shell** above the chat for the existing terminal and Claude/Codex/OpenCode conversation history. Configure the shell launch command and permissions in Settings; an empty command opens an ordinary shell. Agent workspaces retain their split panes, shell tabs and file tabs.

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

- **General**: Restore projects on launch, launch at login, command palette order. Use the up/down arrows to reorder built-in agents and custom commands from the Agents Hub in `Cmd+N`. The first entry is selected by default. Changes persist across launches; Reset restores the default order.
- **Point Guard**: Connect Pi chat to the bridge; configure launch command and skip-permissions for Shell mode
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
