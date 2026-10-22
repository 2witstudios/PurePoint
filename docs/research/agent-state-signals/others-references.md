# Gemini CLI, Cursor CLI, and reference projects

## Which native signals are verified and usable?

### Takeaway
Gemini is usable with a small hook adapter, including a structured question discriminator discovered in stable source. Cursor is partially usable with interactive `stop` hooks; official ACP offers substantially better state and question/permission signals, but switching a PTY integration to ACP is an architectural mode change requiring a client UI.

### Cited Findings
**Gemini scope:** stable npm `@google/gemini-cli@0.64.0` (queried 2026-10-16), official tag `v0.64.0`, commit `4a0749aceac5643e6f85a36d7420314d85e13b52`. All source paths below are relative to [this pinned tree](https://github.com/google-gemini/gemini-cli/tree/4a0749aceac5643e6f85a36d7420314d85e13b52). Current main was also inspected at `ed96faedfd5571f055c44f2c9509d62321d1138d`; do not conflate it with the installed stable version.

| State | Exact signal | Rating and timing | Blocking |
|---|---|---|---|
| Finished | `AfterAgent` | Deterministic **turn-end boundary**, not successful task completion or irrevocable idle. `fireAfterAgentHookSafe` requires outermost active call and no pending tools. Other AfterAgent hooks can force continuation, so a passive observer cannot independently prove no retry follows. | Awaited; other hook outputs can retry/halt. |
| Working | `BeforeAgent` | Deterministic prompt entry, once per prompt ID, before model processing. A later hook may block it; not proof of actual LLM work. `BeforeModel` can provide activity if needed. | Awaited; can block turn. |
| Needs permission | `Notification` with `notification_type:"ToolPermission"`, `details.type` in `edit`, `exec`, `mcp`, `info` | Deterministic confirmation **requested**, emitted after `shouldConfirmExecute` returns details, before state becomes AwaitingApproval and before wait. Exclude `ask_user`. Unknown future detail type should be unknown attention, not permission. | Scheduler awaits hook before presenting/waiting. |
| Asking question | Same `Notification`, but `details.type:"ask_user"` | Source-confirmed deterministic structured user question request. AskUserInvocation returns this confirmation subtype; scheduler passes it through same notification path. Serialized details preserve `type` and `title` but NOT actual questions/options. | Hook awaited, then user confirmation wait. |
| Errored | None dedicated in hooks | Unavailable as structured turn-error signal. `AfterTool.tool_response.error` is tool error, not necessarily terminal turn failure; `SessionEnd.reason` has no error kind. Process nonzero exit can classify process failure, not all turn errors. | n/a |

Sources: [client.ts](https://github.com/google-gemini/gemini-cli/blob/4a0749aceac5643e6f85a36d7420314d85e13b52/packages/core/src/core/client.ts) (`fireBeforeAgentHookSafe`, `fireAfterAgentHookSafe`, `sendMessageStream`); [confirmation.ts](https://github.com/google-gemini/gemini-cli/blob/4a0749aceac5643e6f85a36d7420314d85e13b52/packages/core/src/scheduler/confirmation.ts) (`resolveConfirmation`, `notifyHooks`); [hookSystem.ts](https://github.com/google-gemini/gemini-cli/blob/4a0749aceac5643e6f85a36d7420314d85e13b52/packages/core/src/hooks/hookSystem.ts) (`toSerializableDetails`, `fireToolNotificationEvent`); [ask-user.ts](https://github.com/google-gemini/gemini-cli/blob/4a0749aceac5643e6f85a36d7420314d85e13b52/packages/core/src/tools/ask-user.ts); [official hook reference](https://geminicli.com/docs/hooks/reference/).

Common Gemini payload fields: `session_id`, `transcript_path`, `cwd`, `hook_event_name`, `timestamp`. BeforeAgent adds `prompt`; AfterAgent adds `prompt`, `prompt_response`, `stop_hook_active`; Notification adds `notification_type`, `message`, `details`. The Notification enum has only `ToolPermission`. SessionEnd reasons are `exit`, `clear`, `logout`, `prompt_input_exit`, `other`, not interrupted/error. [types.ts](https://github.com/google-gemini/gemini-cli/blob/4a0749aceac5643e6f85a36d7420314d85e13b52/packages/core/src/hooks/types.ts).

Source-derived illustrative payload (NOT an empirical capture):
```json
{"session_id":"...","transcript_path":"...","cwd":"/sandbox/repo","hook_event_name":"Notification","timestamp":"...","notification_type":"ToolPermission","message":"Tool requires confirmation","details":{"type":"ask_user","title":"Ask User"}}
```
Source-derived completion fields: `{"hook_event_name":"AfterAgent","prompt":"Say hello","prompt_response":"Hello","stop_hook_active":false,...common}`. [hookSystem.ts](https://github.com/google-gemini/gemini-cli/blob/4a0749aceac5643e6f85a36d7420314d85e13b52/packages/core/src/hooks/hookSystem.ts), [hookEventHandler.ts](https://github.com/google-gemini/gemini-cli/blob/4a0749aceac5643e6f85a36d7420314d85e13b52/packages/core/src/hooks/hookEventHandler.ts).

Gemini hook command default timeout is **60000 milliseconds**, customizable per command. General hooks run synchronously; `Notification` ignores flow-control fields but is still awaited at the permission dispatch site. Hook exceptions are caught in `fireToolNotificationEvent`, yet a hung child can delay approval display until timeout. Observer should emit `{}`/exit 0, never exit 2 or control decisions. [runner](https://github.com/google-gemini/gemini-cli/blob/4a0749aceac5643e6f85a36d7420314d85e13b52/packages/core/src/hooks/hookRunner.ts), [official mechanics](https://geminicli.com/docs/hooks/).

**Cursor interactive hooks:** official hook docs describe `stop` input `{status:"completed"|"aborted"|"error",loop_count:0}` and common `conversation_id`, `generation_id`, `hook_event_name`, workspace context. Returning nonempty `followup_message` starts continuation, so a stop observer must not promise final task success. `beforeSubmitPrompt` observes submission but can prevent it. General docs mainly describe IDE Agent Chat; full CLI parity must not be assumed. Source is proprietary. [Official hooks](https://cursor.com/docs/hooks).

The community integration qualifies only native CLI `stop` on macOS arm64 `2026.09.18-9a7762b`, Linux amd64 `2026.09.28-64d2043`, Windows amd64 `2026.10.01-e373342`. This is third-party fixture evidence, not our runtime observation or authoritative support guarantee. It preserves native conversation/generation IDs and intentionally says “stopping”, not completion success; no questions, permissions, errors classification promised. [Pinned qualification](https://github.com/777genius/agent-notifications/blob/0ad3a5053d3f2e68ccabbae5668cfd4b401b0950/docs/cursor-notifications.md), [decoder](https://github.com/777genius/agent-notifications/blob/0ad3a5053d3f2e68ccabbae5668cfd4b401b0950/internal/cursorsource/source.go).

**Cursor ACP official contract (web docs inspected 2026-10-16, no release version attached):** launch `agent acp`; newline JSON-RPC over stdio. Create/load session, send `session/prompt`, receive `session/update`, handle `session/request_permission`, optionally `session/cancel`. The prompt response contains `stopReason`. A JSON-RPC error is a structured failed request, not proof of every possible agent fault. [Official ACP](https://cursor.com/docs/cli/acp).

| State | ACP signal | Rating / blocking |
|---|---|---|
| Working | PurePoint sends `session/prompt`; `session/update` streaming | Deterministic own start and activity; no separate hook needed. |
| Finished | Successful `session/prompt` response, inspect `stopReason` | Deterministic request boundary, distinguish end versus cancel/limits. Need pinned-version capture to confirm complete enum. |
| Needs permission | Incoming `session/request_permission` | Deterministic; CLI blocks if unanswered. Client returns selected allow/reject option. |
| Asking question | Incoming `cursor/ask_question` | Deterministic; explicitly blocking. `toolCallId`, optional `title`, `questions:[{id,prompt,options:[{id,label}],allowMultiple?}]`. Response answered/skipped/cancelled. |
| Errored | JSON-RPC error response / child failure | Deterministic failed request/process only; comprehensive turn-error mapping still unverified. |

`cursor/create_plan` is separately blocking plan acceptance; classify as question/decision or dedicated plan attention. `cursor/task` is a notification of subagent task, with `toolCallId`, `description`, `prompt`, `subagentType`, optional `agentId`, `durationMs`. ACP session identity is created by client and maps directly to a PurePoint launch; do not collapse subagent task notifications into root completion. [Official ACP schemas](https://cursor.com/docs/cli/acp).

### Inferences
- Gemini should be included as supported for source-confirmed start/end-boundary/permission/question subset, capability-gated and version-qualified. Don't install `BeforeTool` merely to infer ask_user: Notification subtype is stronger and closer to actual blocking.
- Cursor should be supported as experimental end-boundary-only in PTY mode, with an optional later full ACP adapter. ACP requests require actual UI response ownership and cannot transparently observe an existing interactive PTY.
- Map native session IDs alongside PurePoint launch/run token and worktree ID, never use basename(cwd) as identity. Hook payload lacks a documented root-parent ID in inspected Gemini common fields; root/nested parity requires tests.

### Gaps
No native Cursor install/runtime test was performed; installed `cursor-agent` and `gemini` were absent from PATH. Gemini CLI was actually installed into `/tmp/pu-signal-discovery/gemini-install` with isolated HOME, npm cache and system config paths. `gemini --version` printed `0.64.0`; `gemini -p 'Say hello'` exited **41** with “Please set an Auth method ... GEMINI_API_KEY, GOOGLE_GENAI_USE_VERTEXAI, GOOGLE_GENAI_USE_GCA”. No actual lifecycle payload was captured. No user's auth or config was read/reused. Consequently normal completion, permission, question, turn error and interrupt empirical cases remain **not run**, not passed. A deterministic fake provider harness similar to community tests or separately authorized fresh sandbox authentication would settle them. Source-derived payloads above must never be labeled observed.

## How should installation, transport and qualification work?

### Takeaway
Use an owned, launch-scoped adapter and bounded local handoff. Avoid copying the reference installers: one has incorrect/stale native payload assumptions and whole-file backup restoration can destroy subsequent user edits.

### Cited Findings
Gemini hooks live in `.gemini/settings.json` nested as `hooks.Event:[{matcher:"...",hooks:[{type:"command",name:"...",command:"...",timeout:...}]}]`; individual event configuration isn't a flat command array. `GEMINI_CLI_HOME` overrides CLI home; system settings/defaults paths have explicit env overrides. [Official config](https://geminicli.com/docs/hooks/reference/), [paths.ts](https://github.com/google-gemini/gemini-cli/blob/4a0749aceac5643e6f85a36d7420314d85e13b52/packages/core/src/utils/paths.ts), [settings.ts](https://github.com/google-gemini/gemini-cli/blob/4a0749aceac5643e6f85a36d7420314d85e13b52/packages/cli/src/config/settings.ts).

Cursor project hooks use `<project>/.cursor/hooks.json`, user hooks `~/.cursor/hooks.json`, and different relative command working directories; ACP needs no notification hooks/config entries, just child process and client. [Official hooks](https://cursor.com/docs/hooks), [ACP](https://cursor.com/docs/cli/acp).

### Inferences
- Proposed Gemini minimum: BeforeAgent, AfterAgent, Notification plus optionally SessionStart/SessionEnd for native identity/process reconciliation. Install owned hook entries with unique names in launch-scoped profile; keep user's defaults intact via deliberate merge if profile isolation is selected. Write adapter/manifest into `.pu` owned location and only three owned registrations; uninstall compare-and-remove own entries, never restore an entire historical backup. If project settings used, honor existing content and git hygiene.
- Proposed Cursor PTY minimum: one `stop` command, version qualification first; BeforeSubmitPrompt only after actual CLI capture. ACP minimum: zero hook files; persistent session bookkeeping in `.pu` and IPC handler integrated with daemon. Do not auto-answer permissions.
- Hook executable reads bounded stdin, attaches inherited launch token, sends to local UDS/datagram or tiny bounded HTTP request, exits success quickly even if listener unavailable. Avoid background grandchildren inheriting hook stdio. Target <100 ms handoff and hard local timeout; choose concrete limits in builder phase. A local spool can improve recoverability but must be bounded, private and not store raw prompt/tool secrets.
- Qualification: for pinned binaries trigger each state plus approval deny, question cancel, Ctrl-C while tool/question/model running, completion followed by hook retry, nested agent, dead/slow listener, duplicate/reordered frames, concurrent worktrees. Assert root state doesn't become idle from nested stop or unknown stale frame.

### Gaps
Gemini permission resolved event is not available in inspected hook set; PostTool/BeforeModel can show resumed progress, but denial/cancel edge cases need empirical confirmation. Cursor CLI hook timeout/fail-open behavior is not source-verifiable; official generic hook semantics need release-qualified tests before promising no stalls. ACP free-form question support is not in documented options-only Cursor schema; retain terminal/manual fallback for free-form prose. Human decision: preserve PTY with limited capabilities or invest in native ACP UI?

## What do the two reference projects really do and may we reuse them?

### Takeaway
Use them as design/test references only. GPL project is far more active and now explicitly distinguishes qualified observation from success; MIT project is young and its native event assumptions are not reliable verification.

### Cited Findings
**777genius/agent-notifications:** pinned `0ad3a5053d3f2e68ccabbae5668cfd4b401b0950`, last commit 2026-10-15 “fix(installer): declare supported managed writer protocols (#488)”. GPL-3.0-or-later license text. Active changelog and extensive platform qualification records, but those records qualify its own exact versions/fixtures. [Pinned repo](https://github.com/777genius/agent-notifications/tree/0ad3a5053d3f2e68ccabbae5668cfd4b401b0950), [license](https://github.com/777genius/agent-notifications/blob/0ad3a5053d3f2e68ccabbae5668cfd4b401b0950/LICENSE).

- Claude registration uses PreToolUse matcher `ExitPlanMode|AskUserQuestion`, Notification matcher `permission_prompt`, Stop, SubagentStop, TeammateIdle, command timeout 30 seconds. AskUserQuestion gives immediate question path; Notification permission maps to generic question in internal notification policy, NOT a clean five-state model. [hooks.json](https://github.com/777genius/agent-notifications/blob/0ad3a5053d3f2e68ccabbae5668cfd4b401b0950/hooks/hooks.json), [router](https://github.com/777genius/agent-notifications/blob/0ad3a5053d3f2e68ccabbae5668cfd4b401b0950/internal/hooks/hooks.go).
- Codex registers Stop, PermissionRequest, experimental PreToolUse matcher `^request_user_input$`, optional SubagentStop. Stop uses final-text heuristic: short failure text => API Error/Session Limit; trailing `?` => Question; else Task Complete. It explicitly says live question tool hook not qualified and does not parse unstable rollout transcript. Continuation Stop suppresses notification. [Codex guide](https://github.com/777genius/agent-notifications/blob/0ad3a5053d3f2e68ccabbae5668cfd4b401b0950/docs/CODEX.md), [hooks](https://github.com/777genius/agent-notifications/blob/0ad3a5053d3f2e68ccabbae5668cfd4b401b0950/hooks/hooks-codex.json).
- Gemini supports only AfterAgent and Notification ToolPermission, tested against exact 0.62.0 via deterministic local provider, not live Google service. Does not promise final-only/root-nested parity. This misses our stable-source ask_user discriminator. Cursor supports only stop with exact versions as above. [Gemini guide](https://github.com/777genius/agent-notifications/blob/0ad3a5053d3f2e68ccabbae5668cfd4b401b0950/docs/gemini-notifications.md).
- Focus-aware delivery is optional Claude/Codex best effort; default false; delay cap 25s due 30s hook timeout; focus/delay applies desktop only, not webhook. Unknown focus degrades to delivery; native terminal pane/session hints are captured in hook process. Useful reference for separate delivery policy, not state classification. [Config](https://github.com/777genius/agent-notifications/blob/0ad3a5053d3f2e68ccabbae5668cfd4b401b0950/docs/CONFIGURATION.md).

**JohnPremKumar/agent-notifier:** pinned `df74903f800b1268a7e478a77fbbbfc758fb4930`, last commit 2026-05-04 “Version Packages (#11)”; packages version 0.1.2. MIT copyright 2026 Johnpremkumar Srinivasan. [Repo](https://github.com/JohnPremKumar/agent-notifier/tree/df74903f800b1268a7e478a77fbbbfc758fb4930), [license](https://github.com/JohnPremKumar/agent-notifier/blob/df74903f800b1268a7e478a77fbbbfc758fb4930/LICENSE).

- Claude: Stop=>TURN_DONE; Notification message regex `/permission/i`=>PERMISSION, `/waiting.*input/i`=>IDLE, rather than structured notification_type.
- Codex expects `event` (not hook_event_name), PermissionRequest=>PERMISSION, Stop=>TURN_DONE. Fixture tests establish its classifier only, not tool dispatch.
- OpenCode registers typed `permission.ask` handler and catchall session.idle, spawning detached notifier with stdio pipe/end/unref; not equivalent to confirmed actual bus permission.asked.
- Gemini expects `event_name` and notification_type `tool_confirmation` or `idle`; this contradicts stable source `hook_event_name` and `ToolPermission`. Installer writes flat `[{command}]`, contradicting official nested matcher/hooks schema. Its Gemini support claim is not evidence of working modern integration.
Sources: [all adapters](https://github.com/JohnPremKumar/agent-notifier/tree/df74903f800b1268a7e478a77fbbbfc758fb4930/packages/core/src/adapters), [Gemini installer](https://github.com/JohnPremKumar/agent-notifier/blob/df74903f800b1268a7e478a77fbbbfc758fb4930/packages/cli/src/lib/installers/gemini.ts).

MIT installer creates one `.agent-notifier.bak` and on uninstall copies it over whole settings file; this can discard later unrelated user changes. [Installer source](https://github.com/JohnPremKumar/agent-notifier/blob/df74903f800b1268a7e478a77fbbbfc758fb4930/packages/cli/src/lib/installers/gemini.ts).

MIT idle gate suppresses while same originating terminal tab focused; permission bypasses gate. Unknown-tab policy defaults fire, configurable gate. macOS terminal introspection varies; don't import platform focus heuristics into authoritative state. [Idle gate](https://github.com/JohnPremKumar/agent-notifier/blob/df74903f800b1268a7e478a77fbbbfc758fb4930/docs/idle-gate.md).

### Inferences
GPL code should not be copied into PurePoint without explicit license compatibility decision; independent adapters based on public contracts avoid dependency/reuse ambiguity. MIT code reuse is possible with notices, but requires technical review and correction; no code copied in this discovery. Maintenance timestamps establish observed recency, not a guarantee of support quality. PurePoint should suppress pushes based on its own selected agent/view, keeping state independent from attention/focus policy.

### Gaps
No complete test-suite audit of either project, no legal review, no live qualification reproduced. OpenCode community mapping details beyond inspected adapters should be cross-checked with the OpenCode research owner, especially question.asked/root filtering in newer GPL plugin.
