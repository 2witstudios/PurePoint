# Scheduling

**Maturity: SEED** | ID Prefix: SCHED | Dependencies: none

## Purpose

Scheduled agent execution: cron-like scheduling that spawns agents at defined intervals. One approach: integrate the scheduler directly into the daemon rather than running it as a separate process.

## Conceptual Model

```
Schedule { name, cronExpression, command, enabled, lastRun, nextRun }
Daemon integrates scheduler directly (no separate process)
```

## Open Questions

? [SCHED-001] How should scheduled task failures be handled — retry with backoff, notify the user, or just log and skip?

? [SCHED-002] Should schedules support dependencies between tasks (e.g., run security review only after dependency audit completes)?
