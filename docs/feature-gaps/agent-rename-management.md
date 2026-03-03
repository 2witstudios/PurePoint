# Agent Rename & Management — Agent & Terminal Rename/Management

## User Story

As a user, I want to rename agents and access richer management actions from a context menu so that I can organize my workspace and perform common agent operations quickly.

## Feature Description

Right-click context menu on agents and terminals with rename, delete (with confirmation), restart, duplicate prompt, view logs, and copy agent ID. Rename persists across sessions. Agent grouping indicators for split-view relationships.

## How ppg-cli Did It

Right-click context menu with rename and delete (with confirmation dialog). Agent groups indicated in split view. Rename updated the display name in the sidebar and persisted to the manifest. Delete killed the tmux session and removed from manifest.

What worked well: Rename was essential for organizing agents beyond their auto-generated names. Confirmation on delete prevented accidental agent loss. Context menu was discoverable and fast.

## PurePoint Opportunity

- **Daemon-backed rename**: Rename persisted in manifest via daemon IPC, ensuring CLI and GUI stay in sync.
- **Richer context menu**: Beyond rename/delete, add restart (respawn with same prompt), duplicate (spawn new agent with same config), view logs (open agent's output history), copy agent ID (for scripting).
- **Bulk operations**: Select multiple agents for batch rename, delete, or restart.
- **Agent metadata**: Tags, notes, or categories for organizing agents in large workspaces.

## Priority

**P1** — Rename is a basic workspace management feature. Its absence is immediately noticeable when working with multiple agents.
