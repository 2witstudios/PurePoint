# Deterministic transcript binding research

Observed 2026-10-10, read-only. No agent/model execution, installs, authentication reads, daemon changes, or personal transcript text copied.

## Existing built-ins

`crates/pu-core/src/types/config.rs` defines Claude, Codex, OpenCode, terminal. Claude already receives a generated `--session-id` and persists it. Codex and OpenCode currently do not persist their session identity and resume through unsafe-for-multiple-consumers `--last`/`--continue` selection. Pi is bundled separately for Point Guard, rather than a built-in pu worker.

## Codex

Installed CLI 0.162.1 supports `--no-daemon`, preserving canonical CODEX_HOME and authentication while disabling shared-server execution. It does not expose a session ID preassignment flag. `codex resume <UUID>` resumes exactly; replace `--last` once identity is bound.

Read-only `lsof -p <pid> -Fn` provides open rollout paths. The examined active worker PID owns several rollout files: one root `source: "cli"`, `thread_source: "user"`; others have `source: {subagent: {thread_spawn: {...}}}`. Subagent `payload.session_id` is the ROOT session ID, whereas `payload.id` is the actual thread ID. Never bind a subagent through `session_id`.

Recommended pure selector: consume process-owned rollout path candidates plus parsed first-record metadata; require valid `type: session_meta`, `payload.source == "cli"`, exact canonical cwd, valid `payload.id`, and exactly one distinct root identity. Persist id and exact path before process exit. For new Codex workers add `--no-daemon` and watch PID/descendant open files immediately after spawn. On ambiguity fail clearly; never newest-file/cwd-only guess. Stopped legacy agents without recorded identity cannot always be reconstructed; an explicit owner-supplied exact binding is necessary rather than inventing one.

SessionStart hooks have exact session_id and transcript_path, but hook trust/configuration makes them a more intrusive option. Writer locks contain no identity beyond filename and use OS file locking, so they do not independently distinguish the owning UI when a server owns several threads.

Sanitized minimal metadata fixture:

```json
{"timestamp":"2026-10-10T00:00:00Z","type":"session_meta","payload":{"id":"00000000-0000-0000-0000-000000000001","session_id":"00000000-0000-0000-0000-000000000001","cwd":"/fixture/project","source":"cli","thread_source":"user","history_mode":"paginated"}}
```

Current response records include message(role,content[].text), function_call(name,arguments JSON string,call_id), function_call_output(call_id,output string), custom_tool_call(name,input string,call_id), custom_tool_call_output(call_id,output string OR content array), agent_message(author,recipient,content). `event_msg` includes duplicated item/user/assistant projection; select one authoritative record family to avoid duplicate text. Exclude encrypted reasoning and metadata/instructions.

Sources: installed `codex --help`, `codex resume --help`; [CLI source](https://github.com/openai/codex/blob/main/codex-rs/tui/src/cli.rs), [SessionStart hook](https://github.com/openai/codex/blob/main/codex-rs/hooks/src/events/session_start.rs), [writer locks](https://github.com/openai/codex/blob/main/codex-rs/rollout/src/writer_lock.rs).

## OpenCode

Installed v2.0.19 explicitly documents `--session <id>` as continue OR create-if-missing. Generate an exact `ses_` prefixed identifier at spawn, persist it, and always resume through `--session <id>`. Current upstream SessionID validates startsWith("ses"); native IDs are `ses_` plus 26 characters (12 hex descending timestamp and 14 random alphanumeric). Do not assume arbitrary UUID works on all supported versions without version-specific verification.

Installed export syntax is `opencode session export <exact-session-id>`; legacy versions use `opencode export <id>`. Running export can start/connect a server. A read-only SQLite reader avoids that side effect.

Observed current storage is `~/.local/share/opencode/opencode.db`, WAL enabled. V2 tables:

```sql
session_v2(id TEXT PRIMARY KEY, directory TEXT NOT NULL, ...);
session_message(id TEXT PRIMARY KEY, session_id TEXT NOT NULL, type TEXT NOT NULL,
                seq INTEGER NOT NULL, time_created INTEGER NOT NULL,
                time_updated INTEGER NOT NULL, data TEXT NOT NULL);
```

Query exact session_v2 id and validate directory, then session_message WHERE session_id=? ORDER BY seq,id. Read in a transaction for consistent snapshots. Data message kinds include user, assistant, synthetic, system, idle, compaction, agent-switched, model-switched. user has text and files. assistant has content[]: text{text}; reasoning{text}; tool{id,name,executed,state{status,input,output/error},time}. Current output includes updates in-place, so an incremental cursor needs a revision/hash boundary or must account for existing message mutation. Legacy SQLite has session/message/part tables; older JSON storage is storage/session, storage/message, storage/part. Detect schema explicitly rather than assuming one format.

V2 export schema is `{info: Session.Info, messages: SessionMessage.Info[]}`. Source [export implementation](https://github.com/anomalyco/opencode/blob/dev/packages/cli/src/commands/handlers/session/export.ts), [transfer schema](https://github.com/anomalyco/opencode/blob/dev/packages/schema/src/session-transfer.ts), [ID schema](https://github.com/anomalyco/opencode/blob/dev/packages/schema/src/session-id.ts), [native ID generation](https://github.com/anomalyco/opencode/blob/dev/packages/schema/src/identifier.ts).

## Existing indexes and other agents

Desktop CodexConversationIndex currently extracts only text after final filename dash (12-character UUID tail), so it cannot reliably map full UUID index IDs. Full UUID should be read from session_meta and verified rather than splitting filename at final dash.

Desktop OpenCodeConversationIndex only supports old JSON session metadata and does not consume current SQLite v2.

Pi sessions contain branches; use native SessionManager leafId/parentId active branch semantics as existing mobile bridge does. Raw JSONL flattening shows abandoned conversation branches. Custom command wrappers cannot be identified solely by configured agent type; report unsupported/capability requirements explicitly. Terminal has no conversation transcript; a bounded, explicitly labeled terminal capture is a different deterministic source, and must never masquerade as an AI transcript.
