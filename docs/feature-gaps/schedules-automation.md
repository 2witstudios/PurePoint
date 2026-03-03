# Scheduled Agent Runs — Scheduled Agent Runs

## User Story

As a user, I want to schedule agents to run automatically on a recurring basis so that routine tasks like code reviews, dependency updates, or test runs happen without manual intervention.

## Feature Description

A scheduling system with calendar UI, cron expression builder, and bindings to swarms or individual prompts. Supports one-time and recurring schedules with variable injection. Schedules persist and execute even when the GUI is not open (via daemon).

## How ppg-cli Did It

Full calendar UI with day/week/month views, a cron expression builder with common presets (hourly, daily, weekly), schedule-to-swarm and schedule-to-prompt bindings, and variable injection for parameterized runs. Schedules stored in project config.

What worked well: The visual calendar made schedule management intuitive. Cron presets lowered the barrier for non-cron-savvy users. Binding schedules to swarms enabled complex automated workflows.

## PurePoint Opportunity

- **Daemon-native cron scheduler**: Rust daemon runs a cron scheduler (tokio-cron-scheduler or similar) that persists across app restarts. Schedules execute as long as the daemon is running, independent of the GUI.
- **CLI schedule management**: `pu schedule list`, `pu schedule add` for terminal-based setup.
- **Run history and logs**: Daemon tracks schedule execution history with success/failure status and links to agent logs.
- **Notification integration**: macOS notifications on schedule completion or failure.

## Priority

**P3** — Powerful feature but requires swarm/prompt infrastructure first. Most users will manually trigger agents initially.
