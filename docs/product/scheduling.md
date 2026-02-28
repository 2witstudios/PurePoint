# Scheduling

**Maturity: SEED** | ID Prefix: SCHED

## Purpose

Scheduled agent execution: cron-like scheduling that spawns agents at defined intervals. One approach: integrate the scheduler directly into the daemon rather than running it as a separate process.

## Conceptual Model

```
Schedule { name, cronExpression, command, enabled, lastRun, nextRun }
Daemon integrates scheduler directly (no separate process)
```
