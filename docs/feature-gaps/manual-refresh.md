# Manual Refresh — Manual Refresh & Data Staleness

## User Story

As a user, I want to manually trigger a data refresh so that I can verify the UI matches the actual state when I suspect staleness or after external changes.

## Feature Description

A manual refresh action (Cmd+R or toolbar button) that forces the UI to reconcile with the current system state. Includes a safety timer to prevent overlapping refreshes and visual feedback during the refresh operation.

## How ppg-cli Did It

Cmd+R triggered a force-refresh of the sidebar and all data sources. A safety timer prevented overlapping refreshes (debounced rapid Cmd+R presses). Refresh re-queried tmux sessions, re-read the manifest, and re-fetched git status.

What worked well: Manual refresh was a reliable escape hatch when the file watcher missed changes or data seemed stale. The safety timer prevented performance issues from rapid refreshing.

## PurePoint Opportunity

- **Daemon subscription model may make this unnecessary**: With IPC event streams, the UI subscribes to state changes and receives updates in real-time. Data staleness should be rare.
- **Force reconcile command**: Instead of "refresh the UI," offer a "reconcile" action that asks the daemon to verify its internal state matches the filesystem and tmux reality, then pushes corrections.
- **Staleness indicator**: Rather than requiring manual refresh, show a subtle indicator if the UI suspects it's out of sync (e.g., daemon heartbeat missed).
- **CLI access**: `pu reconcile` for verifying daemon state from the terminal.

## Priority

**P3** — Lower priority if daemon subscriptions work correctly. Worth implementing as a safety net, but should rarely be needed.
