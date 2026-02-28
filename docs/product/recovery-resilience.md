# Recovery & Resilience

**Maturity: SEED** | ID Prefix: REC

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

## Interfaces

gRPC RPC: Recover (Tier 1)
