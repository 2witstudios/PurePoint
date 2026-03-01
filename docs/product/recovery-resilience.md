# Recovery & Resilience

**Maturity: SEED** | ID Prefix: REC | Dependencies: `architecture/daemon-engine.md`

## Purpose

How PurePoint handles crashes, restarts, and unexpected state: daemon crash recovery, agent process recovery, and state reconciliation.

## Conceptual Model

```
Daemon crashes → agents may still be running
Daemon restarts → reconcile stored state with reality
  For each known agent: check if process still exists
  For unknown processes: adopt or ignore
  Update state to match actual reality
```

## Open Questions

? [REC-001] How should the daemon handle agent processes it doesn't recognize during recovery — adopt them, ignore them, or prompt the user?

? [REC-002] Should recovery be automatic on daemon restart, or require an explicit command?
