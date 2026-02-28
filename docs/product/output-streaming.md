# Output & Streaming

**Maturity: SEED** | ID Prefix: OUT

## Purpose

How agent terminal output is captured, stored, streamed to clients, and summarized. This covers the full pipeline from agent PTY output through to the dashboard's live terminal view and log retrieval.

## Conceptual Model

```
Agent PTY → tmux pane → daemon capture → output_chunks table → gRPC stream → client
                                       → summaries table (auto-generated)
```

## Interfaces

gRPC RPCs: StreamOutput (Tier 2), Logs (Tier 1)
Storage: output_chunks table, summaries table
