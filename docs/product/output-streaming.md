# Output & Streaming

**Maturity: EXPLORING** | ID Prefix: OUT | Dependencies: `architecture/agent-execution.md`

## Purpose

How agent output is captured, stored, streamed to clients, and summarized. Covers the full pipeline from agent process output to the client's live terminal view and log retrieval.

## Conceptual Model

```
Agent process → PTY master fd → reader task (spawn_blocking, 4096-byte chunks)
  → OutputBuffer (1MB circular VecDeque<u8>, RwLock)
    → Logs request (read_tail: last N bytes, UTF-8 lossy)
    → Attach mode (buffered_bytes sent, then live Output messages)
    → Idle detection (idle_seconds, looks_like_shell_prompt)
```

## Research Notes

**Daemon output capture (`pu-engine/src/output_buffer.rs`):** 1MB circular buffer per agent (`DEFAULT_CAPACITY = 1024 * 1024`). Internal structure: `VecDeque<u8>` with `RwLock<BufferInner>` for concurrent read access. On write, if `data.len() > capacity`, excess oldest bytes are drained from the front. Tracks `last_write: Instant` for idle detection.

**API for output retrieval:**
- `Request::Logs { agent_id, tail }` — returns last `tail` bytes as UTF-8 lossy string via `read_tail(n)`.
- `Request::Attach { agent_id }` — enters interactive mode. Returns `AttachReady { buffered_bytes }`, then streams live `Output { agent_id, data }` messages (hex-encoded PTY bytes).
- `Request::Input { agent_id, data }` — sends keystrokes to agent's PTY master fd (hex-encoded bytes). Only valid during attach mode.

**Prompt detection (`looks_like_shell_prompt`):** Reads last 256 bytes, converts to UTF-8 lossy, strips trailing `\n`/`\r`, checks if result ends with `"$ "`, `"% "`, `"# "`, or `"> "`. Used by `agent_monitor::effective_status()` for idle detection.

**No-daemon phase (macOS app):** SwiftTerm's grouped tmux session IS the output viewer — there is no separate capture layer. The terminal view connects directly to the agent's tmux session via `tmux new-session -t {session}`, displaying live output with full terminal emulation (colors, cursor positioning, alternate screen buffer).

## Open Questions

? [OUT-001] How should output chunking work — fixed-size chunks, line-based, or semantic boundaries (e.g., tool call boundaries)?
(Current implementation uses raw byte chunks from PTY reads, no semantic parsing.)

? [OUT-002] What triggers summary generation — agent completion, periodic intervals, or on-demand when a client requests it?
(Not yet implemented.)
