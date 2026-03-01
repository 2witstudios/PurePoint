# Recovery & Resilience

**Maturity: SEED** | ID Prefix: REC | Dependencies: `architecture/daemon-engine.md`

## Purpose

How PurePoint handles crashes, restarts, and unexpected state: daemon crash recovery, tmux session recovery, agent restart, and state reconciliation.

## Conceptual Model

```
Daemon crashes → agents may still run in tmux
Daemon restarts → reconcile DB with tmux reality
  For each known agent: check if tmux pane exists
  For unknown tmux panes: adopt or ignore
  Update DB to match actual state
```

## Open Questions

? [REC-001] How should the daemon handle tmux panes it doesn't recognize during recovery — adopt them, ignore them, or prompt the user?

? [REC-002] Should recovery be automatic on daemon restart, or require an explicit `pu recover` command?

## Interfaces

gRPC RPC: Recover (Tier 1)
