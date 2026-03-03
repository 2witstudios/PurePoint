# Multi-Project Support — Multi-Project Workspace Support

## User Story

As a user, I want to have multiple projects open simultaneously so that I can work across related codebases without closing and reopening projects.

## Feature Description

Support for multiple projects open at the same time, with quick switching between them. Each project maintains its own agent pool, sidebar state, and terminal sessions. Aggregate views (like dashboard) can show cross-project activity.

## How ppg-cli Did It

Multiple projects open simultaneously with Cmd+1-9 switching between them. Per-project tab system with independent sidebar and terminal state. Dashboard aggregated stats across all open projects.

What worked well: Quick keyboard switching between projects was fast. Per-project isolation meant one project's agents couldn't interfere with another's. Aggregate dashboard stats gave a unified view across all work.

## PurePoint Opportunity

- **Daemon-native multi-project**: The Rust daemon already has project isolation in its architecture. Multiple project roots can be managed concurrently with independent agent pools.
- **Flexible presentation**: Multi-project could be implemented as tabs (like ppg-cli), separate windows (more macOS-native), or a unified sidebar with project grouping.
- **Cross-project orchestration**: Daemon can coordinate agents across projects — e.g., a swarm that touches multiple repos.
- **CLI parity**: `pu project list`, `pu project switch <name>` for terminal access to the same multi-project state.

## Priority

**P2** — Many users work across multiple repos. Currently requires separate PurePoint windows with manual management.
