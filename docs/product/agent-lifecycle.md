# Agent Lifecycle

**Maturity: CONVERGING** | ID Prefix: AL | Dependencies: `architecture/agent-execution.md`

## Purpose

The complete lifecycle of an AI agent from spawn to completion: creation, monitoring, status detection, output capture, restart, and cleanup.

## Conceptual Model

Process liveness and observed turn state are separate contracts:

- `AgentStatus::Running`: process has no observed exit status. It may be idle or waiting for input.
- `AgentStatus::Broken`: process exited/disappeared, including exit code zero. It does not describe a turn outcome.
- `signal` (optional): launch-bound activity, requests pending by ID, terminal turn outcome and observation freshness. Unsupported or unavailable observation is Unknown; output silence is never completion.

Alive state (`is_alive()`): Running. Terminal process state: Broken.

## Decisions

! [AL-001] Process liveness is `Running`/`Broken`, determined solely by observed process exit (`effective_status(exit_code)` in `pu-engine/src/agent_monitor.rs`). No shell prompt or output-inactivity heuristic establishes turn activity, attention, or success. The optional passive signal contract in `pu-core/src/signals.rs` keeps activity, pending attention, terminal outcomes and observation health separate without changing process/PTY/session ownership or launch/input/resume/shutdown semantics.

## Signal contract

- Given owned launch identity, observations must match agent, optional worktree, launch ID/generation and exact native root session. Child, stale-generation, unknown-session, duplicate and reordered events cannot transition the root. Cwd is diagnostic only.
- Given authoritative start/outcome evidence, activity and terminal outcome are updated separately from process status. A definitive outcome is completed, errored or interrupted; attempted stops and recoverable provider/tool errors never establish completion/failure. A confirmed next start supersedes the prior outcome without resolving pending requests.
- Given a current native idle observation with no pending attention or terminal outcome, clients display Idle. A newly opened or resumed idle session does not need a completed turn to establish idle activity; Idle never implies Finished. Unknown remains the fallback for unavailable evidence or unknown activity.
- Before Codex creates its first thread, a current native idle packet from the exact owned PTY may establish launch-only Idle with an empty native session ID and authoritative evidence. This exception permits no pending requests or outcomes, never overrides a known different native session, and cannot establish Working. Once a root binds, all normal native-session identity and source-loss gates apply. A suspended agent displays Suspended from lifecycle metadata, without claiming a turn outcome; unavailable machines still gate the presentation.
- Initial idle does not require a stored conversation: Claude may use its unchanged, strictly bound idle PID registry before a transcript has ever existed; OpenCode may use a verified native service's exact-session not-found responses and stable absent execution before that assigned session is materialized. Neither fallback applies after observed storage disappears or on resume. In-progress JSONL appends may retain a verified complete prefix, but no partial record establishes facts; inode/owner changes, malformed complete records, terminal byte loss and truncation (including unread data) remain invalidation boundaries.
- Given authoritative pending attention, retain permission/question requests by ID, including simultaneous requests during background work. Intent is insufficient. Resolve matching IDs only; notification delivery/read and error acknowledgement are not resolution. Actionability defaults false separately on macOS/iOS until exact-request discovery, supported response and authoritative resolution are qualified.
- Given unavailable, stale, disconnected, lost or dead observation, clients gate retained facts to Unknown and withdraw stale attention counts. Current requires schema version 1 and `last_received_at_ms <= now < expires_at_ms` (Unix milliseconds). Invalid/replayed/intent evidence cannot renew that lease. Loss/disconnection requires authoritative full reconciliation; stale pending requests cannot regain freshness from unrelated events. Process death invalidates observation irreversibly for that launch and does not manufacture success.
- Given fresh authoritative resolution for an exact retained request after lease expiry, remove only that request without renewing the old lease, evidence, activity or outcome. Other stale requests remain stale and block unrelated progress. After stale requests are all removed, a fresh definitive outcome/new start can establish current facts; a fresh new pending request can establish only its own attention, clearing old stale activity/outcome to Unknown/None. This supports awaited UI with no model turn start and never infers Finished from an answer. Internal confirmed-active-turn correlation survives that Unknown projection so a later authoritative matching terminal outcome can settle an open, previously started turn; new UI alone grants no eligibility to never-started or closed turns.
- Given a semantic no-op (known current/closed turn start, duplicate pending request, already-resolved request or inapplicable outcome), ignore before replay-history capacity failure without renewing freshness or retaining another event ID. A first unseen resolution may retain a bounded safety tombstone, but does not renew unrelated facts.
- Given bounded capacity exhaustion, fail closed as lost rather than silently evicting live requests or replay-safety tombstones. Limits are explicit, validated and finite. Transport authentication, bounded parsing and private launch capability remain engine responsibilities; the reducer does no I/O.

`AgentStatusReport.signal`, `AgentInventoryEntry.signal` and `WorktreeEntry.agents[id].signal` (via `AgentEntry`) carry the same snake_case `SignalSnapshot`. Existing worktree/agent manifest keys remain camelCase. Missing `signal` decodes as None; signal enums decode future values as Unknown. An unsupported signal schema fails closed at projection. Persisted snapshots are not restart reconciliation evidence: `read_manifest()` strips root and worktree signals, and `write_manifest()` omits them from a cloned persistence copy without changing live caller data. Generic `AgentEntry`/worktree/status serde retains live signals; no manifest schema change is required. See `crates/pu-core/tests/fixtures/signals-status-v1.json` and `signals-inventory-v1.json` for sanitized, synthetic wire examples, not live adapter qualification.

## Research Notes

**Current baseline reconciliation (2026-10-17):** `pu-core/src/types/agent.rs` has `Running`/`Broken`; historical `streaming`, `waiting`, `spawning`, `idle`, `suspended` map to Running and historical terminal values map to Broken. `effective_status()` takes only exit code. Output idle seconds remain diagnostics. Earlier Streaming/Waiting and prompt-timeout descriptions were historical, not the current liveness contract.

**Evidence qualification:** the independently reviewed discovery in [agent state signal research](../research/agent-state-signals/discovery.md) distinguishes source-confirmed contracts from runtime captures. Claude Stop/permission intent alone do not establish final idle/actionable pending input; OpenCode terminal execution events must be distinguished from retryable tool failures; generic pi confirm is question/input, not permission. Adapters retain unsupported capabilities rather than adopting private OpenCode servers, Codex app-server or Cursor ACP ownership. This correction does not advance spec maturity or claim complete adapter qualification.

**Status from daemon (macOS app):** The desktop app queries status from the daemon via IPC (`status` request → `status_report` response). A manifest file watcher triggers re-queries on file changes (300ms debounce), but the data source is the daemon, not the manifest file directly.

**Terminal connection (macOS app):** The daemon streams output via the IPC `Attach`/`Output` protocol. The desktop app connects via `DaemonWorkspaceService`, which issues an `Attach` request for the agent and receives `Output` messages with live PTY data. Multiple viewers can attach to the same agent simultaneously.

## Open Questions

! [AL-002] Agents support suspend/resume — CLI exposes this as `pu bench` (suspend) and `pu play` (resume). Maps to `Request::Suspend`/`Request::Resume` in the IPC protocol with `SuspendTarget::Agent(id)` or `SuspendTarget::All`. Suspended agents have `suspended: true` and `suspended_at` timestamp in the manifest. Implemented in `pu-engine/src/engine.rs` (suspend/resume handlers) and `pu-cli/src/commands/bench.rs` + `play.rs`.

**App quit behavior:** When the macOS app quits, it sends `Request::Shutdown` to the daemon. The daemon kills all agent processes and exits. Agent state (command, worktree, last status) is persisted in the manifest. On next app launch, agents can be re-spawned from the saved state. Standalone CLI mode does not auto-shutdown — agents persist until explicitly killed or the daemon is stopped.
