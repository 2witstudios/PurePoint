# Output & Streaming

**Maturity: SEED** | ID Prefix: OUT | Dependencies: `architecture/agent-execution.md`

## Purpose

How agent output is captured, stored, streamed to clients, and summarized. Covers the full pipeline from agent process output to the client's live terminal view and log retrieval.

## Conceptual Model

```
Agent process → output capture → storage → stream to client
                                         → summary generation
```

## Research Notes

**No-daemon phase:** In the initial no-daemon architecture, SwiftTerm's grouped tmux session IS the output viewer — there is no separate capture layer. The terminal view connects directly to the agent's tmux session via `tmux new-session -t {session}`, displaying live output with full terminal emulation (colors, cursor positioning, alternate screen buffer). Output capture/storage will be added when the daemon is implemented.

## Open Questions

? [OUT-001] How should output chunking work — fixed-size chunks, line-based, or semantic boundaries (e.g., tool call boundaries)?

? [OUT-002] What triggers summary generation — agent completion, periodic intervals, or on-demand when a client requests it?
