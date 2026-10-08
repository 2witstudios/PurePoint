# Pi Mobile standalone contract

Maturity: SPECIFIED. Scope: standalone iPhone chat → Mac bridge → vanilla Pi. No dependencies on PurePoint engine, desktop or IPC. Authorizing brief: Jono, 2026-10-08.

## Purpose and conceptual model

The Mac process owns one persistent Pi RPC child. The phone is one authenticated controller and a replaceable view. Pi owns session storage and execution. Phone drafts and submission receipts are recovery aids, never a second authoritative transcript.

## Research and decisions (2026-10-08)

Research progressed SEED (lifetime / session authority questions), EXPLORING (upstream and harness inspection), CONVERGING (SDK versus RPC, log replay versus snapshots), DECIDED, then SPECIFIED here before implementation.

! [MOB-001] Use a child RPC process rather than an in-process agent — isolates runtime and keeps stdout/stderr distinct. Upstream published `@earendil-works/pi-coding-agent` 1.1.0 verified with npm registry. Lockfile pins all dependencies. Upstream inspected at ce950d78f424dcaf9f5d6a03ce80ab141130eb1d; published package docs/source are final authority. Sources: [package](https://github.com/earendil-works/pi/blob/ce950d78f424dcaf9f5d6a03ce80ab141130eb1d/packages/coding-agent/package.json), [RPC](https://github.com/earendil-works/pi/blob/ce950d78f424dcaf9f5d6a03ce80ab141130eb1d/packages/coding-agent/docs/rpc.md), rpc-mode.ts / rpc-types.ts, [commands](https://github.com/earendil-works/pi/blob/ce950d78f424dcaf9f5d6a03ce80ab141130eb1d/packages/coding-agent/docs/rpc-commands.md).

! [MOB-002] Use revisioned authoritative snapshots rather than a durable network event ledger — a single child and controller need reconnect state, not another transcript store. Walk get_entries parentId from leafId; do not flatten abandoned branches. Display raw branch history with compaction/branch-summary markers; this is reading history, not rebuilding model context. Pi alone applies firstKeptEntryId/context edits to model context. Read-only session browsing uses native SessionManager.inMemory with bounded file entries; native SessionManager.listAll enumerates metadata. Switching uses RPC switch_session. Sources: [session format](https://github.com/earendil-works/pi/blob/ce950d78f424dcaf9f5d6a03ce80ab141130eb1d/packages/coding-agent/docs/session-format.md), session-manager.ts.

! [MOB-003] Serialize mutations; use epoch/run target for Stop — clear_queue returns text, abort waits for idle, and agent_end can precede automatic continuation. agent_settled is the idle boundary. Dialog responses bypass mutation serialization so extension commands cannot deadlock. No automatic prompt replay. Unknown protocol/operations reject visibly.

! [MOB-004] Bind only explicit tailnet IPv4/IPv6 or loopback, authenticate in WebSocket HTTP upgrade with an owner-provided bearer secret — Tailscale encrypts traffic even for ws inside the tailnet. Optional owner TLS supports wss. Reject public/wildcard binds. Second controller is rejected; disconnect never aborts Pi. No credential generation or global installation.

! [MOB-005] Preserve vanilla resources — no provider overrides or no-skills flag. Pi discovers ~/.agents/skills (including symlinks). Discover a pu skill in installed Codex skill/plugin roots and add its path explicitly. get_commands validates that skills loaded. Clear PU_AGENT_ID and PU_PROJECT_ROOT. Cross-project pu commands must route with a per-command PU_PROJECT_ROOT or explicit cwd (installed pu has no general --project-root flag). Source: skills.md, configuration.md, skills.ts. Read-only PageSpace harness precedent at 0cc42713700b5a779ba88feb62054c583c0f7eba: bin/pagespace.mjs uses argument arrays and extension preload; extensions/pagespace.ts replaces filesystem/provider behavior and its launcher disables native skills. None of those restrictions/replacements are adopted.

! [MOB-006] Native SwiftUI with system typography — prose-first, blue user bubbles, adaptive system backgrounds, 20pt transcript margins, 24pt turn spacing, Dynamic Type, selectable Markdown and monospaced copyable code. Blue #2563EB / light #F2F2F7 / white #FFFFFF / dark #000000 / secondary #8E8E93. Use native multiline editing and explicit buttons so newline/IME never submits. No decorative dashboard or protocol controls in conversation. Reviewed against brief: native text and restrained activity are the distinguishing surface; omit generic cards/gradients.

## Requirements

- REQ-MOB-001: Given phone disconnect/backgrounding, should keep Pi running and reconnect to authoritative branch history plus partial response/tools without replay or duplicates.
- REQ-MOB-002: Given fragmented stdout, should split only LF, preserve Unicode/UTF-8, correlate IDs, separate diagnostics, bound records/requests/client buffers and visibly fail on child exit.
- REQ-MOB-003: Given idle/busy composition, should submit complete text with explicit Send/Steer/After reply, show started/queued/handled receipts separately from completion and preserve later edits and uncertain submitted text.
- REQ-MOB-004: Given Stop, should clear queued text before abort, retain canceled text, target the observed epoch/run, reject stale commands and never kill pu workers.
- REQ-MOB-005: Given native sessions, should create/list/browse/resume; require idle for switching or offer explicit Stop then switch; respect tree and compaction semantics.
- REQ-MOB-006: Given extension dialogs, should present confirm/select/input/editor and correlate answers/cancellations; retain pending dialogs across reconnect, expire timed requests, and preserve composer drafts on set_editor_text.
- REQ-MOB-007: Given missing Pi/model/skills, should provide setup guidance without reading or printing secret files; preserve native provider/auth and use explicit cwd/argument arrays.
- REQ-MOB-008: Given invalid auth/version/second controller, should reject; bound inbound/outbound network data and never expose arbitrary RPC commands or filesystem paths as operations.

## Interface

WebSocket upgrade: Authorization: Bearer OWNER_SECRET, /v1. First record `{version:1,id,op:"sync"}`. All requests include version, id (unique UUID), op. Operations: sync, sessions, history(sessionId), send(text,mode:send|steer|after,epoch), stop(epoch,runId), new(epoch), resume(epoch,sessionId), answer(dialogId,value|confirmed|cancelled).

Replies: `{type:"receipt",id,ok,data?,error?}`. Command timeouts are uncertain, never retried automatically. Canceled queue text is retained in bounded bridge-lifetime recovery snapshots. Cached request IDs are bound to one bridge lifetime; repeated IDs reject rather than redispatch. Snapshots: `{type:"snapshot",version:1,epoch,revision,runId,busy,sessionId,title,messages,tools,queue,canceled,dialogs,notices,error?}`. Epoch rotates on session changes/runtime restart. IDs for rows derive from native role/timestamp or entry IDs; live assistant final replaces partial. Messages contain id,role,text,activity,error. Text is bounded and truncation marked. Snapshots replace the phone projection; revision suppresses old delivery. Read-only history returns a separate projection without selecting it.

## Edge cases and limits

One controller, one active session. No bridge crash durability for request receipts; Pi sessions survive, partial in-flight generation may not. Restart creates a fresh session unless PI_MOBILE_SESSION is supplied. Stop cannot cancel an extension handler before it starts a future independent run; stale epochs/runs reject. Dialogs may be canceled by Pi timeouts; bridge expires them. TUI-only custom widgets are unsupported by Pi RPC; string widgets/status/notices are surfaced. Images display a text placeholder in this text-first slice. At most 500 visible history rows, 64KiB per text, 100 tools, 32 dialogs, 8MiB RPC records, 16MiB history files, 1MiB network requests and 4MiB client backlog. Older complete history remains in Pi native files. Tailnet membership/access policy and Mac availability remain owner-managed.
