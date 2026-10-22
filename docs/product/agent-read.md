# Agent Conversation Reads

**Maturity: SPECIFIED** | ID Prefix: READ | Dependencies: `architecture/ipc-api.md`, `architecture/agent-execution.md`, `architecture/module-structure.md`

## Purpose

Read persisted agent activity without terminal rendering artifacts. The daemon
owns source resolution; CLI and future native clients share pure normalization
and pagination. This does not change agent execution into headless mode.

## Research and decisions

Research progressed from source/identity questions through repository and installed
runtime inspection to the following scoped decisions. Existing PTY byte tails
cannot represent conversation records. Desktop transcript readers provide useful
schema evidence but silently skip errors and do not establish exact worker binding.

- ! [READ-001] Read exact native sessions through per-runtime adapters — never
  select the latest transcript by cwd, timestamp or directory order. Claude has
  a generated session ID; OpenCode supports preassigned `--session`. Codex exposes
  no preassignment, but a private `--no-daemon` process owns its root rollout file;
  root session metadata distinguishes delegated threads. Persist verified bindings.
- ! [READ-002] Pure functions normalize records and paginate a snapshot — filesystem,
  SQLite and process discovery remain engine effects. IDs derive from native IDs
  and block positions, never wall-clock time or random values at read time.
- ! [READ-003] A cursor binds agent/source and the consumed normalized prefix —
  projection changes, compaction, replacement or mutable tool updates invalidate
  it visibly. Initial reads return newest records; incremental reads return next
  records in order. A block is one record, so pagination can divide a message.
- ! [READ-004] Terminal workers use an explicitly labeled terminal-text source —
  maintain emulator state from process start, distinguish process generations,
  detect retained-history loss, and never claim terminal text is an AI transcript.
- ! [READ-005] Retain `pu logs` as deprecated raw diagnostics — stderr guidance
  preserves legacy stdout/JSON and `pu attach` stays an interactive terminal.

Installed schema evidence: Claude JSONL message content/tool blocks; Codex rollout
`session_meta` and `response_item` records (avoid duplicate `event_msg` messages);
OpenCode v2 SQLite `session_v2`/`session_message` and legacy message/part storage;
Pi native JSONL tree messages. Sources: existing `TranscriptParser.swift`,
`CodexConversationIndex.swift`, bridge native/session code, and runtime transcript-binding research
(2026-10-10). Private format changes require fixtures
and explicit schema diagnostics, not a terminal fallback.

## Interfaces

`pu read AGENT [--limit N] [--since CURSOR] [--session EXACT-ID] [--json]` sends daemon `Read`.
Limit defaults to 50 and ranges from 1 to 200. Optional project scope constrains
ownership. Response identifies agent, runtime, native session/source kind,
normalized records, next cursor, older/more indicators and incomplete-tail state.
Record kinds: user, assistant, tool_call, tool_result, notice and terminal.
Tool records preserve call ID/name, input/output and error status where available.
Text rendering removes control sequences even inside tool results.

Snapshots are bounded to 64 MiB source bytes, 1 MiB per native record and bounded
discovery. Oversized/corrupt/unreadable sources fail explicitly. Incomplete trailing
JSONL waits for a complete record; malformed complete records fail. Cursor strings
are bounded, validated data and never filesystem authority. Native IDs cannot
contain path separators. Unknown runtime types are explicitly unsupported.

## Requirements

- REQ-READ-001: Given Claude, Codex, OpenCode or configured Pi activity, should
  return normalized records from the exact session, including tool activity.
- REQ-READ-002: Given two workers sharing cwd, should preserve distinct identity
  through spawn/read/resume and reject ambiguous legacy discovery.
- REQ-READ-003: Given a terminal worker, should return labeled readable terminal
  text with process-generation and retained-range checks.
- REQ-READ-004: Given pagination, should return stable ordered records without
  losing blocks; updates/rewrites/branch changes should reject stale cursors.
- REQ-READ-005: Given partial writes, malformed records or unavailable storage,
  should distinguish pending tails from explicit errors, never empty success.
- REQ-READ-006: Given supported source overrides, should retain effective storage
  roots; conflicting identity flags should reject rather than bind another session.
- REQ-READ-007: Given unsupported custom runtime or incompatible daemon, should
  return actionable diagnostics without restarting the running daemon.
- REQ-READ-008: Given terminal control sequences inside transcript content,
  should produce readable output without executing those controls.

## Proof obligations

RED/GREEN pure-function tests cover each parser, control cleanup, native identity,
pagination, mutation and partial records. Real temp files/databases and real CLI/IPC
cover effects/routing, suspended agents and daemon restart. Independent review and
workspace fmt/test/clippy cover the composed candidate. Isolate Cargo targets;
never overwrite installed binaries or run macOS app builds from this session.

Runtime compatibility: native adapters target the inspected Claude JSONL, Codex CLI 0.162.1 (requires `--no-daemon`) and OpenCode 2.0.19 schemas, with older OpenCode SQLite and JSON file readers. Codex binding begins immediately on spawn using owned descriptors; until a root rollout exists identity is pending. A worker that exits before capture remains explicitly unbound and requires an exact owner-provided native ID. No sampling algorithm can reconstruct that ownership from timestamps deterministically. Terminal text is available only while its capture handle is retained; stopped/reaped captures fail explicitly.
