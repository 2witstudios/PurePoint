# Scheduling

**Maturity: SEED** | ID Prefix: SCHED | Dependencies: none

## Purpose

Scheduled agent execution: recurring or time-based task spawning. A nightly security review, a weekly dependency audit — results waiting when you check in.

## Conceptual Model

```
Schedule: named recurring task (timing, command, enabled/disabled, run history)
```

## Open Questions

? [SCHED-001] How should scheduled task failures be handled — retry with backoff, notify the user, or just log and skip?

? [SCHED-002] Should schedules support dependencies between tasks (e.g., run security review only after dependency audit completes)?

? [SCHED-003] Should the scheduler be part of the daemon or a separate process?
