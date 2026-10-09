# Project Channel and Review Screens

**Maturity: SPECIFIED** | ID Prefix: CH | Dependencies: `architecture/ipc-api.md`, `architecture/desktop-app-integration.md`

## Purpose

Provide a quiet place to follow agent work: project conversation alongside compact worktree rows, full-width worktree review with cumulative committed changes, and an optional shared channel. Agents post intentionally through CLI; normal messages and mentions never send terminal input or start work.

## Source and authority

The owner approved the quiet v2 layout and v3 channel refinement, then requested implementation on 2026-10-09. The selected source is `designs/agent-watch/v3/manifest.json` and its SHA-256-pinned HTML snapshots. Implement native SwiftUI/AppKit controls, not the HTML comparison frame or sample data. Worktree screens contain no terminals. Existing file editing and workspace terminals remain available.

## Scoped research and decisions

Research used repository `pu-core/manifest.rs` (fs4 lock + atomic replace), engine request dispatch, newline JSON IPC, `DaemonClient`, `ProjectState`, `GitService`, `DiffState`, and sidebar routing.

- ! [CH-001] Store a versioned per-project `.pu/channel.json`, serialized under a persistent lock and atomically replaced — matches existing local manifest storage without introducing a database decision for unrelated domains. Reject unsupported future versions; preserve history across engine restarts and worktree cleanup.
- ! [CH-002] Keep all writes in the daemon and CLI as a thin client — concurrent CLI/app writes share validation and identity handling.
- ! [CH-003] Use bounded, cursor-based reads plus UI polling of channel revision while visible — avoids new streaming protocol complexity and prevents terminal/trigger coupling. An edited/reaction-changed message must refresh even if its creation sequence does not change.
- ! [CH-004] Separate creation sequence (stable unread cursor) from store revision (changes on edits/reactions). Replies get their own creation sequence and parent ID. Return enough parents for loaded replies and support explicit older history.
- ! [CH-005] Resolve agent sender by manifest membership, preserving sender name/type/worktree/branch in the message at creation; developer clients use stable local human identity — historical identities survive agent removal. UI must never invent completion from process-running state.
- ! [CH-006] Use merge-base-to-HEAD for branch result and base-exclusive log for history; disclose actual base and errors. No branch default silently assumes main if another base was recorded.

Alternative storage considered: JSONL append is cheap but complicates edits/reactions; SQLite is capable but broader than existing file-store patterns. Alternative UI delivery considered: status stream integration couples unrelated lifecycles; periodic bounded revision reads are sufficient for human-paced updates. Git file-system events alone miss linked gitdirs and recursive working-file changes; bounded foreground polling supplements resolved metadata watches.

## Channel wire contract (snake_case)

Payload types:
- `ChannelAuthor`: `id: String`, `name: String`, `kind: String` (`human`/`agent`), `agent_type: String?`, `worktree_id: String?`, `branch: String?`.
- `ChannelReference`: `kind: String` (`commit`/`pr`), `value: String`, `label: String?`.
- `ChannelReaction`: `emoji: String`, `author_ids: [String]`.
- `ChannelMessage`: `id: String`, `sequence: UInt64`, `parent_id: String?`, `author: ChannelAuthor`, `text: String`, `created_at: ISO8601 String`, `edited_at: String?`, `references: [ChannelReference]`, `reactions: [ChannelReaction]`.

Requests:
- `channel_read`: `project_root`, optional `after: UInt64`, optional `before: UInt64`, `limit` default 100/max 200, optional `query`, optional `known_revision: UInt64`. Reads return oldest-to-newest within the selected window; no cursor selects latest window. Revision optimization only applies to unfiltered latest reads.
- `channel_send`: `project_root`, optional `agent_id`, `text`, optional `parent_id`, `references` default [].
- `channel_edit`: `project_root`, optional `agent_id`, `message_id`, `text`.
- `channel_react`: `project_root`, optional `agent_id`, `message_id`, `emoji` default thumbs-up, `active: Bool`.

Responses:
- `channel_history`: `messages`, `revision: UInt64`, `latest_sequence: UInt64`, `has_more: Bool`, `oldest_sequence: UInt64?`, `unchanged: Bool`.
- `channel_message`: `message`, `revision: UInt64`.
- Use existing `error` responses for invalid identities, parents/references, missing messages, oversized input, unsupported store versions, I/O errors and edit ownership violations.

Agent sender IDs are local claimed identities checked against that project's manifest, not credentials; existing IPC socket permissions are the trust boundary. Never take an arbitrary author display name from clients. Human author ID must be the same between local CLI and app. Local clients sharing the same OS account can claim registered agent IDs; this is documented and not remote authentication.

Limits: text 1–16,000 UTF-8 bytes (whitespace-only rejected); references at most 8 with bounded fields; read limit at most 200; query at most 256 chars. Persist errors are not converted into empty successful history. Idempotent reaction activation avoids retries toggling twice. Edit may change text only. Parent must be a top-level message in this project; no deeply nested threads.

## CLI

`pu channel send <text> [--reply-to ID] [--commit SHA] [--pr NUMBER] [--json]`; `pu channel read [--since SEQUENCE] [--before SEQUENCE] [--limit N] [--search TEXT] [--json]`; `pu channel edit ID <text> [--json]`; `pu channel react ID [--remove] [--json]`. Resolve project via explicit `--project-root`, `PU_PROJECT_ROOT`, or Git common directory so subdirectories and linked worktrees route correctly. Identity uses `PU_AGENT_ID` when present, otherwise local human. Commands must not prompt or inject input into agents.

## Given/should requirements

- REQ-CH-001: Given explicit CLI/app posting, should persist one shared project message with stable identity and worktree context without terminal input, triggers or wakeups.
- REQ-CH-002: Given engine restart and concurrent writers, should retain ordered, nonduplicated history without lost updates or partial JSON.
- REQ-CH-003: Given history larger than one read window, should support older/newer cursors and search without silently dropping parent context.
- REQ-CH-004: Given an own message, should allow text editing; given another sender, should reject editing.
- REQ-CH-005: Given reply/reaction, should persist and refresh it across CLI/app reads; reaction retries should be idempotent.
- REQ-CH-006: Given project selection, should show short rows for all worktrees plus root checkout alongside the shared channel.
- REQ-CH-007: Given worktree selection, should default to branch changes with uncommitted/commit/file views and optional channel, without terminals.
- REQ-CH-008: Given commit creation, should keep cumulative branch result visible after the working tree becomes clean.
- REQ-CH-009: Given staged, unstaged and untracked files, should show their actual patches in the correct groups and count each local path once in the overview.
- REQ-CH-010: Given a comparison base, should show its name; invalid/unrelated bases should produce visible errors.
- REQ-CH-011: Given newer local changes or remote PR updates, should refresh while preserving selections, expanded state, drafts and readers' position.
- REQ-CH-012: Given full/embedded channel, should use shared native message grouping, identities, timestamps, references, unread navigation/search, replies/reactions/editing and accessible hover/focus actions.
- REQ-CH-013: Given composition, should preserve drafts across navigation/reload, disable empty send, use Enter/Shift+Enter predictably without submitting IME composition, offer keyboard mentions and inline-code rendering, and show retryable failure without silently replaying uncertain sends.
- REQ-CH-014: Given saved read cursor, should synchronize sidebar/timeline unread state without marking messages read merely by opening an older timeline.
- REQ-CH-015: Given missing daemon/GitHub or corrupt persistence, should retain available evidence and visibly report unavailability rather than empty success.

## Verification

Real temp Git repositories and channel stores, engine dispatch tests, CLI parsing/routing/output, Swift wire compatibility/state tests, native component typechecks where safe. Run Rust fmt/test/clippy in an isolated target directory; never install or overwrite the active daemon. Per repository safety rules, app build/tests and rendered native UI checks remain owner-run in Xcode and are explicit outstanding acceptance evidence.
