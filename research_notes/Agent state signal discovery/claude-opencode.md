# Claude Code and OpenCode

## What signals are verified, at which versions?

### Takeaway
Claude has documented lifecycle hooks, but several apparently useful notifications are delayed or narrowly scoped. OpenCode v2 has a stronger lifecycle than the familiar v1 plugin documentation suggests; ship explicit v1/v2 adapters instead of treating the schemas as interchangeable.

### Cited Findings

Research date 2026-10-16. Local `claude --version`: **2.1.300**. Local `opencode --version`: **v2.0.28** (Mach-O standalone executable). Claude implementation source is not publicly available, so documentation claims below are **doc-verified**, not source-verified. Official rolling docs retrieved on this date do not attach a minimum release to every event; do not invent introduction versions. [Claude reference](https://code.claude.com/docs/en/hooks).

OpenCode inspected releases: v1.2.27 commit **4ee426ba549131c4903a71dfb6259200467aca81**; installed release v2.0.28 commit **026b106bc5d41b679ffc413264229c340871e4d3**. Also cloned main **65932e870ca50d21886f9d0ee89c6603bac34a65** (CLI package 2.0.32). `git diff v2.0.28 HEAD` is empty for `packages/schema/src/{form,permission,session-event}.ts` and `packages/core/src/session/execution.ts`, so those schemas/timing were checked against the installed release tag. [v2 release source](https://github.com/anomalyco/opencode/tree/026b106bc5d41b679ffc413264229c340871e4d3), [v1 release source](https://github.com/anomalyco/opencode/tree/4ee426ba549131c4903a71dfb6259200467aca81).

#### Claude matrix (official rolling reference, local version 2.1.300)

| State | Signal | Rating and limitation | Blocking |
|---|---|---|---|
| Finished/idle | `Stop` | Deterministic responding boundary, **not irrevocable idle**: another Stop hook can reject stopping, `/goal` can continue; no Stop on interrupt or API error. Payload `stop_hook_active`, `last_assistant_message` plus common fields. | Synchronous default; hook decision can continue agent. |
| Permission | `PermissionRequest` | Exact permission-decision boundary; not proof a dialog is waiting: also runs headless auto-denial, other hook/SDK host can decide immediately. **Does not fire for sandbox network requests**. Payload `tool_name`, `tool_input`, optional `permission_suggestions`; **no `tool_use_id`**. | Synchronous, may approve/deny. |
| Permission fallback | `Notification` / `permission_prompt` | Deterministic delayed notification, ~6 seconds inactivity (every keystroke defers); covers sandbox network prompt on >=2.1.246. | Notification cannot block by decision, but command duration still matters unless async. |
| Question | `PreToolUse` matcher `AskUserQuestion` | Deterministic question-tool attempt, **not guaranteed displayed/waiting**: later hooks may answer/deny/defer. Payload `tool_use_id`, `tool_name`, `tool_input.questions[]`; each has question/header/options/multiSelect. | Synchronous default; can intercept tool. |
| MCP question | `Elicitation` | Exact MCP input request boundary, not arbitrary assistant prose; other hook may answer programmatically. `ElicitationResult` closes it before response returns to MCP server. Fields include `mcp_server_name`, message/mode and form schema or URL depending mode. | Hook can answer/decline/cancel request. |
| Error | `StopFailure` | Deterministic **API-error** turn ending, not all crashes/tool errors. Runs instead of Stop. Fields `error`, optional `error_details`, optional `last_assistant_message` (rendered error text). | Output/exit ignored (except terminalSequence); synchronous execution still may delay. |
| Working | `UserPromptSubmit` | Exact turn-submission boundary before processing, also autonomous turns in current docs; not proof model call began, prompt hook can reject. Use `PreToolUse` as continued activity, `PostToolUse`/`PostToolUseFailure` to resolve question/tool pending state. | Can reject prompt; synchronous default. |
| Interrupt | no general Stop equivalent | Docs explicitly exclude user interruption from Stop. Process lifecycle needed for exit; keep interrupted distinct from successful finished. | N/A |

All table claims: [Claude hook reference](https://code.claude.com/docs/en/hooks#hook-events); [Stop](https://code.claude.com/docs/en/hooks#stop); [PermissionRequest](https://code.claude.com/docs/en/hooks#permissionrequest); [questions](https://code.claude.com/docs/en/hooks#tools-that-require-user-interaction); [StopFailure](https://code.claude.com/docs/en/hooks#stopfailure); [notification timing](https://code.claude.com/docs/en/hooks#notification).

Common input includes `session_id`, `transcript_path`, `cwd`, `hook_event_name`; most hooks include `permission_mode`; tool/subagent context can include `agent_id`, `agent_type`. Session hooks vs per-turn hooks are explicitly separated. Tool hooks run inside subagents too; `SubagentStop` identifies child completion and **must not idle the root**. [Common fields](https://code.claude.com/docs/en/hooks#common-input-fields), [hook locations/subagents](https://code.claude.com/docs/en/hooks#hook-locations).

Notifications must not be generalized: `idle_prompt` waits ~60 seconds after responding, no typing, no running background agent, and does not fire while waiting for automatic usage-limit resume. `agent_needs_input` and `agent_completed` largely describe **background sessions with terminal agent view open**; completed also includes failures. `elicitation_dialog` and URL counterpart wait ~6 seconds inactivity. [Notification](https://code.claude.com/docs/en/hooks#notification).

#### OpenCode v1.2.27 matrix (source verified)

Events have `{type,properties}`. [Bus payload/dispatch](https://github.com/anomalyco/opencode/blob/4ee426ba549131c4903a71dfb6259200467aca81/packages/opencode/src/bus/index.ts).

| State | Event | Fields and timing | Rating |
|---|---|---|---|
| Finished idle | `session.status` status.type=`idle`; legacy `session.idle` | `sessionID`, `status`; idle emitted for cancellation and terminal errors too. | Deterministic idle, not successful outcome. |
| Permission | `permission.asked`; close `permission.replied` | Asked `{id,sessionID,permission,patterns[],metadata,always[],tool?:{messageID,callID}}`; reply `{sessionID,requestID,reply:once\|always\|reject}`. Asked published after pending map insertion and before `Deferred.await`; rules already allowing/denying emit no Asked. | Deterministic pending request. |
| Question | `question.asked`; close `.replied`/`.rejected` | Asked `{id,sessionID,questions[],tool?}`; question fields question/header/options/multiple?/custom?; reply `{sessionID,requestID,answers:string[][]}`; reject sessionID/requestID. Pending map before Asked, then await Deferred. | Deterministic pending question. |
| Error | `session.error` | `{sessionID?:...,error}`; processor terminal nonretry error emits error then idle; context overflow also emits error though recovery/compaction may continue. Plugin-load errors can emit without sessionID. | Exact error occurrence, **not invariably ended turn**. Correlate idle/terminal assistant result. |
| Working | `session.status` status.type=`busy` / `retry` | Busy while prompt loop/model starts; retry `{attempt,message,next}`. | Deterministic runtime state; busy persists while awaiting permission/question. |

Sources: [question service](https://github.com/anomalyco/opencode/blob/4ee426ba549131c4903a71dfb6259200467aca81/packages/opencode/src/question/service.ts), [permission service](https://github.com/anomalyco/opencode/blob/4ee426ba549131c4903a71dfb6259200467aca81/packages/opencode/src/permission/service.ts), [status](https://github.com/anomalyco/opencode/blob/4ee426ba549131c4903a71dfb6259200467aca81/packages/opencode/src/session/status.ts), [processor error/retry dispatch](https://github.com/anomalyco/opencode/blob/4ee426ba549131c4903a71dfb6259200467aca81/packages/opencode/src/session/processor.ts), [cancel sets idle](https://github.com/anomalyco/opencode/blob/4ee426ba549131c4903a71dfb6259200467aca81/packages/opencode/src/session/prompt.ts).

`permission.ask` is a **plugin handler key** in plugin type definitions, not a bus event. No runtime callsite for `Plugin.trigger("permission.ask"...)` was found in inspected v1 source; do not install this deprecated-looking typed handler as the approval observer. [Plugin types](https://github.com/anomalyco/opencode/blob/4ee426ba549131c4903a71dfb6259200467aca81/packages/plugin/src/index.ts).

#### OpenCode v2.0.28 matrix (source verified at matching installed release)

| State | Event | Exact data | Rating |
|---|---|---|---|
| Finished | `session.execution.succeeded` | `{sessionID}` | Deterministic terminal **busy-period** observation covering coalesced drains; use idle state separately. |
| Permission | `permission.asked`; `permission.replied` | Asked `{id,sessionID,action,resources[],save?,metadata?,source?:{type:"tool",messageID,id},message?}`; replied `{sessionID,requestID,reply}`. | Exact pending request, though create API can create pending without blocking caller; tool assert waits. |
| Question | `form.created`; `.replied` / `.cancelled` | Created `{form:{id,sessionID,title,fields[],metadata?}}`; replied `{id,sessionID,answer}`; cancelled `{id,sessionID}`. | Exact pending form; `Form.ask` blocks on Deferred, `Form.create` alone does not. Forms can have `sessionID:"global"` for MCP; do not assign those arbitrarily to an agent. |
| Error | `session.execution.failed` | `{sessionID,error}` | Deterministic terminal busy-period failure. Individual `session.tool.failed`/`session.step.failed` do not mean turn ended. |
| Working | `session.execution.started` | `{sessionID}` | Exact execution claim/start. |
| Interrupt | `session.execution.interrupted` | `{sessionID,reason:user\|shutdown\|superseded\|inactivity}` | Deterministic; shutdown preserves claim for continuity, others cancel jobs/release claim. |

Sources: [lifecycle schemas](https://github.com/anomalyco/opencode/blob/026b106bc5d41b679ffc413264229c340871e4d3/packages/schema/src/session-event.ts), [execution dispatch](https://github.com/anomalyco/opencode/blob/026b106bc5d41b679ffc413264229c340871e4d3/packages/core/src/session/execution.ts), [permission schema](https://github.com/anomalyco/opencode/blob/026b106bc5d41b679ffc413264229c340871e4d3/packages/schema/src/permission.ts), [permission wait](https://github.com/anomalyco/opencode/blob/026b106bc5d41b679ffc413264229c340871e4d3/packages/core/src/permission.ts), [form schema](https://github.com/anomalyco/opencode/blob/026b106bc5d41b679ffc413264229c340871e4d3/packages/schema/src/form.ts), [form timing](https://github.com/anomalyco/opencode/blob/026b106bc5d41b679ffc413264229c340871e4d3/packages/core/src/form.ts).

V2 envelopes are `{id,type,created,data,location?,metadata?,durable?:{aggregateID,seq,version}}`; lifecycle events durable, permission/forms ephemeral. V1 `{properties}` parser will silently miss V2 `{data}`. V2 plugin is `define({id,setup(context)})` with `context.event.subscribe` via EventDomain, not V1 return `{event(...)}` API. Server event handler provides SSE and 15-second heartbeat. [Envelope](https://github.com/anomalyco/opencode/blob/026b106bc5d41b679ffc413264229c340871e4d3/packages/schema/src/event.ts), [plugin context](https://github.com/anomalyco/opencode/blob/026b106bc5d41b679ffc413264229c340871e4d3/packages/plugin/src/promise/plugin.ts), [SSE handler](https://github.com/anomalyco/opencode/blob/026b106bc5d41b679ffc413264229c340871e4d3/packages/server/src/handlers/event.ts).

### Inferences
Use V2 explicit outcome events ahead of deprecated session.idle. Distinguish busy-period from single submitted message: coalescing means one lifecycle observation can cover multiple drains. The state machine should retain sets of pending permission/form IDs, overlay them on busy runtime state, and clear only matching IDs. Session IDs include child sessions; retrieve parent relationships instead of allowing child succeeded to idle root. Track source session plus root agent mapping.

### Gaps
No end-to-end OpenCode model-driven run was completed; source verification is not empirical verification. V2 plugin subscription backpressure/failure handling and exact external API installation/config recipe need a bounded sandbox spike. V2 form.created by itself cannot prove the creator used blocking ask. Claude Stop cannot prove final idle in presence of other hooks/goal continuation; true final TUI idle and escape-to-cancel prompt lifecycle lack a verified immediate hook.

## What empirical evidence exists, and what remains unverified?

### Takeaway
A bounded isolated Claude test confirmed registration and actual start payloads only. No actual finished/approval/question/error/interrupt payload should be claimed from this test.

### Cited Findings
Local artifacts: `/tmp/pp-claude-test/probe.py`; sandbox `/var/folders/np/tf0z_dyj4hgfdq118gh3wrkr0000gn/T/pp-claude-home-g6kkc_dn/`. Probe created throwaway repo, set **HOME and CLAUDE_CONFIG_DIR**, removed inherited ANTHROPIC/CLAUDE/API_KEY/TOKEN variables, supplied deliberately fake key and loopback HTTP server returning 401. Used `--settings <sandbox>/settings.json --setting-sources '' -p 'say hi' --output-format json --max-turns 1`. `SessionStart`, `UserPromptSubmit`, `Stop`, `StopFailure`, `SessionEnd` each used `cat >> <sandbox>/hooks.jsonl`, timeout 2s. Process was killed by subprocess after **35 seconds**. Actual hooks:

```json
{"session_id":"fabfd8bd-90e5-40cb-89f0-ff953a9d0cea","transcript_path":"<sandbox>/claude/projects/<encoded-repo>/fabfd8bd-90e5-40cb-89f0-ff953a9d0cea.jsonl","cwd":"<sandbox>/repo","hook_event_name":"SessionStart","source":"startup"}
{"session_id":"fabfd8bd-90e5-40cb-89f0-ff953a9d0cea","transcript_path":"<sandbox>/claude/projects/<encoded-repo>/fabfd8bd-90e5-40cb-89f0-ff953a9d0cea.jsonl","cwd":"<sandbox>/repo","prompt_id":"0dbf7665-df78-42d0-9e18-79e5d9b06a57","permission_mode":"auto","hook_event_name":"UserPromptSubmit","prompt":"say hi"}
```

These are actual captured payloads with paths abbreviated for portability. Timeout/forced kill is **not** a verified user-interrupt test. No StopFailure observed before timeout. Evidence source is local probe/log (no remote URL). Version commands were read-only against installed tools; no CLI installation was needed. No user configuration/credentials read or changed.

### Inferences
Next empirical matrix should use deterministic mock model response sequences for text completion, question tool, permission tool and HTTP error; add interactive PTY tests with explicit default/manual permission mode and cancel keystroke. Validate before promising feature support. The loopback401 timeout may involve startup/retries; do not infer error hooks absent.

### Gaps
Claude normal completion, permission, question, terminal API error, and real interrupt: unverified. OpenCode all five plus interrupt: unverified. Each requires actual model or compatible mock/API-driven sandbox test. Testing headless alone misses interactive dialog behavior; captures must identify mode, settings, CLI version, raw payload and event order.

## How should builders integrate safely with PurePoint?

### Takeaway
Do not replace process liveness with one overloaded status enum. Add a separate turn/attention surface, launch identity, and versioned adapters with fail-open bounded delivery.

### Cited Findings
Claude hooks default synchronous, default command/http/mcp timeout **600 seconds** in current docs (30 on UserPromptSubmit and model-switch events; 10 on MessageDisplay). `SessionEnd` shared budget 1.5s, configurable up to 60. `async:true` command hooks do not wait or apply decisions; their timeout is not enforced. `onFailure` defaults continue; avoid `onFailure:block`, avoid exit2 and decision output entirely for telemetry. Hook timeout generally allows flow but may have event-specific behavior. [Hook configuration](https://code.claude.com/docs/en/hooks#common-fields), [timeouts](https://code.claude.com/docs/en/hooks#timeouts), [async](https://code.claude.com/docs/en/hooks#run-hooks-in-the-background).

Claude supports per-launch `--settings <file-or-json>` and `--plugin-dir`; local help verified these flags. Settings hooks merge rather than replacing other settings; policy can disallow local hooks. Prefer launch-owned file under `.pu` passed with `--settings`, preserving normal user config; use `--setting-sources ''` only in empirical sandboxes, not user runs. Remove launcher argument and delete owned file for uninstall. Standard hook locations are `.claude/settings{,.local}.json`, user settings, plugin hooks; no reason to mutate those for PurePoint-spawned sessions. [Hook locations](https://code.claude.com/docs/en/hooks#hook-locations).

OpenCode v1 plugin event callbacks are invoked without awaiting returned promises in Plugin.init; other named handlers are awaited sequentially by trigger. Synchronous throws/CPU work still threaten agent process; async rejected promises must be caught. v1 config supports `OPENCODE_CONFIG`, `OPENCODE_CONFIG_DIR`, inline `OPENCODE_CONFIG_CONTENT`; plugin arrays concatenate and file:// plugins can load from external path. Config-dir loading can install support package/lockfile; do not pretend it is zero-disk. [Plugin dispatch](https://github.com/anomalyco/opencode/blob/4ee426ba549131c4903a71dfb6259200467aca81/packages/opencode/src/plugin/index.ts), [configuration loading](https://github.com/anomalyco/opencode/blob/4ee426ba549131c4903a71dfb6259200467aca81/packages/opencode/src/config/config.ts), [official plugin docs](https://opencode.ai/docs/plugins/).

Current PurePoint source at c6a4cde9f17051cc05fc745140571561e7767642: `crates/pu-engine/src/agent_monitor.rs::effective_status` returns Broken for any exit code (including0), Running otherwise; `crates/pu-core/src/types/agent.rs::AgentStatus` only Running/Broken, legacy waiting/idle decoded to Running. `engine/agent_lifecycle.rs` resume spawn injects `PU_AGENT_ID`, `PU_PROJECT_ROOT`, removes CLAUDECODE; there is already launch identity to extend. **Spec drift:** docs/product/agent-lifecycle.md AL-001 still describes 30s/shell-prompt Waiting detection, contradicted by actual current source. [Local agent monitor](../../crates/pu-engine/src/agent_monitor.rs), [types](../../crates/pu-core/src/types/agent.rs), [lifecycle](../../crates/pu-engine/src/engine/agent_lifecycle.rs), [stale spec](../../docs/product/agent-lifecycle.md).

Existing docs/plans/agent-watch-implementation.md and agent-watch-handoff.md concern approved UI/channel/Git work, not hook telemetry; do not overwrite them or reinterpret their completed delivery as this scope. [Existing implementation plan](../../docs/plans/agent-watch-implementation.md), [handoff](../../docs/plans/agent-watch-handoff.md).

### Inferences
Builder manifest candidates: additive turn/attention types in pu-core types/agent.rs plus protocol/manifest serialization; launch adapter configuration and scoped launch token in engine agent_lifecycle.rs and actual spawn path; daemon receiver/identity validation plus event reducer; notification policy/UI decoding; replace stale AL-001 text after maturity/research review. Keep liveness Running/Broken separate from working/idle/waiting_question/waiting_permission/error/unknown and last outcome interrupted/completed/error. Assign each launch immutable generation + random capability token; session registration binds native session to PU_AGENT_ID and generation, not cwd alone. cwd is supplemental and can change. Reject stale generation/unbound cross-agent events, deduplicate and sequence where upstream provides sequence. Child pending attention may bubble to root UI; child completion must not end root.

Minimum Claude proposed observation set: SessionStart, UserPromptSubmit, Stop, StopFailure, PreToolUse matcher AskUserQuestion, matching PostToolUse and PostToolUseFailure, PermissionRequest, Notification matcher permission_prompt (network gap), Elicitation/ElicitationResult, SessionEnd; SubagentStart/Stop if showing child aggregation. Permission resolution has no verified universal dedicated event: next execution/tool result clears specific pending inferred identity; cancellation may require reconciliation and mark unknown instead of stale permanent blocking. Do not auto-answer any prompts.

Transport: small local executable reads stdin, validates bounded JSON, writes bounded launch-owned queue or uses short-timeout Unix socket; always exits0 with empty stdout. Network listener failures must not enter tool decision channel. PurePoint ingestion/delivery must never wait on push delivery. Prefer OpenCode out-of-process event stream where lifecycle already owned; V2 durable lifecycle cursor enables replay, ephemeral pending attention must reconcile list snapshots after reconnect. If plugin needed, bounded enqueue and caught failures; never await network in execution handlers. Receiver auth token must not be exposed in logs/payloads; Unix socket may avoid opening HTTP interface. Async Claude command hooks avoid awaiting but require helper-enforced deadline and prevent background stdout pipe retention.

Installation/uninstallation: PurePoint launch manifest records exact owned files and flags/env. Claude generated settings/helper under `.pu` and per-launch registry; pass --settings; uninstall removes only manifest-owned artifacts/flag. OpenCode v1 generated config references versioned plugin file via file://, pass OPENCODE_CONFIG with merge-aware treatment if user already sets it; no blind replacement. OpenCode v2 prefer external server subscription (no plugin user-config write), but private/standalone server ownership and endpoint auth must be validated. Stop adapters/unsubscribe, remove only owned config/plugin/state; preserve user cache/package manifests. Build gate must prove round-trip config preservation and listener-down bounded latency for each supported version.

### Gaps
Exact V2 launch isolation/config/plugin load/unload commands and on-disk side effects not empirically established. V2 server local service reuse could merge sessions and identity; private server should be explicitly chosen and tested. Parent/root session relation and API snapshots require deeper schema tracing before implementation. No license code reuse was proposed or copied; upstream source read for behavior only. Push authorization, focus gating, noise/cooldown policy and handling unsupported adapters are human/product decisions. Unsupported/question-prose fallback should show unknown or idle with optional heuristic “may need input”; never label a trailing question mark deterministic blocked.
