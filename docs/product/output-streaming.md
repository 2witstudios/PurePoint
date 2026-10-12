# Output & Streaming

**Maturity: CONVERGING** | ID Prefix: OUT | Dependencies: `architecture/agent-execution.md`

## Purpose

How agent output is captured, stored, streamed to clients, and summarized. Covers the full pipeline from agent process output to the client's live terminal view and log retrieval.

## Conceptual Model

```
Agent process → PTY master fd → reader task (spawn_blocking, 4096-byte chunks)
  → OutputBuffer (4MB circular VecDeque<u8>, RwLock)
    → Logs request (read_tail: last N bytes, UTF-8 lossy)
    → Attach mode (buffered_bytes sent, then live Output messages)
    → Idle detection (idle_seconds, looks_like_shell_prompt)
```

## Research Notes

**Daemon output capture (`pu-engine/src/output_buffer.rs`):** 4MB circular buffer per agent (`DEFAULT_CAPACITY = 4 * 1024 * 1024`). Internal structure: `VecDeque<u8>` with `RwLock<BufferInner>` for concurrent read access. On write, if `data.len() > capacity`, excess oldest bytes are drained from the front. Tracks `last_write: Instant` for idle detection, plus `last_content_write` for spinner-aware idle detection.

**API for output retrieval:**
- `Request::Logs { agent_id, tail }` — returns last `tail` bytes as UTF-8 lossy string via `read_tail(n)`.
- `Request::Attach { agent_id }` — enters interactive mode. Returns `AttachReady { buffered_bytes }`, then streams live `Output { agent_id, data }` messages (hex-encoded PTY bytes).
- `Request::Input { agent_id, data }` — sends keystrokes to agent's PTY master fd (hex-encoded bytes). Only valid during attach mode.

**Prompt detection (`looks_like_shell_prompt`):** Reads last 256 bytes, converts to UTF-8 lossy, strips trailing `\n`/`\r`, checks if result ends with `"$ "`, `"% "`, `"# "`, or `"> "`. Used by `agent_monitor::effective_status()` for idle detection.

## Open Questions

? [OUT-001] How should output chunking work — fixed-size chunks, line-based, or semantic boundaries (e.g., tool call boundaries)?
(Current implementation uses raw byte chunks from PTY reads, no semantic parsing.)

? [OUT-002] What triggers summary generation — agent completion, periodic intervals, or on-demand when a client requests it?
(Not yet implemented.)

## Researched LAN streaming prerequisites

### [OUT-001] Bounded raw-byte chunks and explicit retention gaps

**Researched: 2026-10-11.** `output_buffer.rs::read_from` currently copies every
retained byte after an offset, potentially 4 MiB per subscriber, and silently
advances an older cursor to retained data. This is suitable for the existing local
API but cannot by itself provide the LAN plan's bounded-frame and gap contract.
The ring already separates PTY capture from observers and should remain the
source of best-effort replay.

Option A: bounded byte ranges with explicit available-start, next offset, total
written and missing range. The copy is bounded by the caller's chunk budget;
consumers incrementally parse terminal escapes/UTF-8 across frame boundaries.
Option B: buffer complete lines or semantic events. This improves readability for
logs but cannot represent arbitrary interactive terminal bytes without additional
unbounded buffering or a second raw stream. Recommend A for PTY transport;
conversation records already provide a separate structured read API.

The bridge must bind cursors to daemon incarnation and exact agent/run before
using a ring offset. A future offset must reject rather than rewind silently.
Receiver grants remain necessary before copying/enqueuing output. A bounded
range API alone does not establish aggregate budgets or control responsiveness.

### [OUT-002] Summaries remain an explicit consumer operation

**Researched: 2026-10-11.** Existing `Logs`, terminal snapshots and native
conversation reads are deterministic retrieval operations; this pipeline has no
summary scheduler or model-call owner. Option A is on-demand summarization of a
bounded authorized snapshot, with cost and partial-history visibility at the
consumer. Option B is automatic completion/periodic summarization, which adds
background execution, credentials, durable jobs and retention policy. Narrow to
these two choices; recommend A if summaries are requested. The LAN bridge does
not require generating summaries and this research does not authorize new model
calls. This is independent of the raw streaming implementation.

## LAN range requirements

- REQ-OUT-001: Given retained terminal output and a valid offset, should copy at
  most the requested chunk budget and return the next offset plus snapshot total.
- REQ-OUT-002: Given an offset older than retained bytes, should identify the
  unavailable range explicitly before returning a bounded retained suffix.
- REQ-OUT-003: Given a future offset or zero chunk budget, should reject rather
  than reset a cursor or allocate an unbounded response.
- REQ-OUT-004: Given slow observers, capture should continue into the bounded
  worker-local ring independently; bridge fanout must enforce additional global
  and per-subscriber budgets and validate incarnation/run identity.
