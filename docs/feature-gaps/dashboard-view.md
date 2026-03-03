# Dashboard View — Project Dashboard & Activity Overview

## User Story

As a user, I want a project dashboard that shows me aggregate agent activity, recent commits, and project health at a glance so that I can understand what's happening across all my agents without clicking into each one.

## Feature Description

A dedicated dashboard view that serves as the landing page when opening a project. Displays aggregate statistics (active agents, total commits, recent activity), per-agent status cards, commit history with heatmaps, and live git status. Provides a high-level overview before diving into individual agent work.

## How ppg-cli Did It

Rich dashboard with aggregate agent stats, per-project cards, commit heatmaps, recent commits list, and live git fetch on a 60-second polling interval. Served as the default view when opening a project. Stats were computed on the main thread from tmux session queries and git log parsing.

What worked well: The dashboard gave immediate situational awareness. Commit heatmaps made activity patterns visible. Per-agent cards provided quick status without navigating away.

## PurePoint Opportunity

- **Daemon-driven real-time stats**: Instead of 60s polling, the Rust daemon can push live activity updates via IPC subscription. Agent status changes, new commits, and activity metrics stream to the UI as they happen.
- **Rust git integration**: Use gitoxide for commit data extraction — faster than shelling out to git, with structured access to commit metadata.
- **Reactive UI**: SwiftUI's observation model means dashboard cards update automatically as daemon state changes, with no manual refresh cycle.

## Priority

**P2** — Important for user experience but not blocking core agent workflows. The sidebar already provides basic agent status; the dashboard adds the aggregate view.
