# Passive agent signal adapters

Observation is separate from process liveness. The daemon keeps its existing PTY,
input, native session, resume and shutdown paths. No adapter answers requests or
returns policy decisions. Research remains in `reports/Agent state signal discovery.md`
and `research_notes/Agent state signal discovery/`.

| Runtime | Activation | Facts and qualification limits |
| --- | --- | --- |
| Claude Code 2.1.300 | Read-only `claude agents --json` plus incremental exact root transcript; no injected hooks/settings | Exact owned PID/session/cwd/interactive kind/start epoch, exact CLI/transcript version. Native busy means Working only for an accepted root user turn that has not settled; native waiting category is joined to a single unresolved native tool-use ID for Question or Permission. Native idle plus structural assistant end_turn/API-error metadata on the current parent chain means Completed/Errored. Rejected submit attempts cannot create/revive Working; delayed older-branch output cannot close a newer turn. Ambiguous/missing IDs, future versions, child/foreign records and incomplete/mixed views withdraw observation. Real interactive CLI plus local fake Anthropic provider passed question→answer→completion, manual Write approval→completion, and API400→error through the production observer; no real credentials or config copied. Interrupt outcome classification, concurrent dialogs, MCP elicitation and full Engine/native-app route qualification remain open. |
| pi 1.3.0 | Launch-only `--extension`; first private frame must match owned PTY PID before exact native session binding | Native agent_start, final settled outcome, UI prompt pairs. Generic confirm means question, never permission. RPC confirm/select/input pairs observed with this extension/helper, no model credentials; model lifecycle/terminal notification routes not qualified. Native session changes within a process are not silently rebound. |
| OpenCode v2.0.28 | Read-only authenticated queries of the existing managed service; exact launch-assigned root session | Private owner-mode0600 XDG state registration, loopback URL, exact service version/PID, root ID and directory. GET-only active/session/permission/form snapshots project Working, native terminal outcomes and pending request IDs; execution/session views are checked around the query. No plugin injection, service start/restart, --standalone, ownership migration or user configuration changes. Production observer passed real interactive Write approval→actual sandbox file→completion, question→native First answer→completion, and API400→error with a local fake provider. Harness services use distinct disposable config ports. Public Engine Spawn, deny/cancel/interrupt/child/resume and native-app route qualification remain open. Explicit custom --server/--standalone routes are not yet observed. |
| Gemini 0.64.0 | Fixture-only normalizer | Pin 4a0749aceac5643e6f85a36d7420314d85e13b52. Start/end boundaries and ToolPermission subtype intents; no proved launch-only settings/exact root session installation or universal resolution/error hooks. |
| Codex CLI 0.163.0 | Existing PTY OSC7501 plus incremental rollout held by the exact owned native PID; no hook/config/ownership changes | Actual native idle/working/blocked packets, exact CLI root/session/cwd/version and native turn/call IDs. Working requires an accepted native task_started record; definitive typed terminal outcomes remain settled despite later auxiliary UI activity. Generic blocked is classified only with one outstanding known root call; unknown or concurrent calls fail closed. Native task_complete.error distinguishes API failure from completion. Explicit root terminal records resolve requests; source truncation/replacement/byte loss invalidates observation. Production observer passed real isolated interactive permission→approval→completion, question→answer→completion, and API400→error using a local fake provider and a disclosed raw-PTY-to-OutputBuffer test seam. A spoofed OSC packet in assistant content was stripped by the native UI. Public Engine Spawn, deny/cancel/interrupt/child/resume and native-app routes still require qualification. |
| Cursor | Fixture-only stop-boundary normalizer | CLI parity and install/timeout unknown; no ACP ownership, no generic IDE hooks presented as CLI proof. |

No qualification or release key is enabled by this change. Attention flags remain
false without an exact owner-attested native/platform route. RPC UI proof does not establish
interactive terminal discovery/response, platform notification routes, or physical
Mac/iOS acceptance. All production attention notifications remain gated.

## Bounds and failure behavior

* Private launch directory (0700), exact owned settings/extension files (0600),
  short private Unix datagram socket directory (0700), random per-launch token.
  Capabilities are inherited only by the launch; fresh spawns strip inherited
  `PU_SIGNAL_*` material. Native session, launch ID and worktree bind reducer facts.
* CLI observer bypasses CLI parsing, daemon discovery, background plugin updates,
  Tokio startup and remote transport. Empty stdout/stderr, successful observer exit.
  Stdin deadline 50ms, input/frame maximum 256KiB, nonblocking local datagram send,
  process timer 100ms from helper entry. No retries or remote waits.
* First executable validation after relinking on this macOS host measured roughly
  275–350ms **before helper entry**. Launch preparation preflights the helper with
  a 500ms timeout and disables observation for old/incompatible helpers. This
  warms normal hook use but cannot guarantee an OS scheduling/cold-start bound.
  The proposed universal 100ms wall-clock hook budget is **not proved**. Independent review also observed a first cold helper test exceeding 500ms (warm rerun passed); this remains a recorded qualification failure. The new Claude adapter installs no hooks.
* pi relay is nonawaited FIFO, at most 64 queued events/one child, 100ms child timer. Every pi frame must retain the owned PID, exact session, finite monotonic source sequence and current timestamp; only observer_bound sequence 1 can register it.
  Overflow/timer loss reports an explicit loss marker when possible; sequence gaps
  invalidate observation. Final dropped events cannot always be recovered; no
  heartbeat silently restores safety records. Freshness expires after 30s.
* 64 pending requests/512 event IDs per reducer. Disconnected/lost state needs a
  real full authoritative snapshot; no adapter fabricates Reconciled from hooks.
  A dead owned process invalidates observation, never manufactures success.
  Restart retains no trusted in-memory observation; absent signal means unknown.
* Installation failure does not block the native launch. Only exact unchanged
  owned files are removed; later edits/unrelated files are preserved. No user
  settings/auth copies, user-home config writes, daemon binary installs, app builds,
  new outbox/gateway/APNs or execution-mode changes.
* OpenCode polling runs independently of the PTY and never replies to requests.
  Native HTTP requests disable proxies/redirects, cap each response at1MiB,
  have a1s request timeout and2s overall snapshot deadline, and poll at500ms.
  A failed snapshot withdraws observation; a subsequent complete native snapshot
  can reconcile it. Owned PTY exit irreversibly withdraws that launch's observer.
  Each snapshot holds at most64 pending IDs and refers only to the assigned root.

* Claude observation runs outside the PTY and issues only native version/list queries.
  A native poll has a5s overall deadline and at most1MiB of JSON output per command.
  Transcript reads are incremental, at most8MiB per poll/1MiB per record, and retain
  only IDs, tool categories and structural outcomes. Existing histories larger than
  the per-poll budget are caught up incrementally. A timed-out Claude poll retains
  its single in-flight reader and private replay/settlement state. Native registry is sampled on
  both sides, and transcript inode/length must still match afterward. Truncation,
  replacement or invalid root history latches observation unavailable for the launch.
  Up to64 unresolved tool IDs are retained. Queries do not change user settings,
  input, resume, process/session ownership or permission policy. No hook/helper is
  needed for Claude. Initial idle without a terminal result does not invent Finished.

## Checks and reproduction

```
cargo test -p pu-engine --lib signals::
cargo test -p pu-cli --test signal_helper
cargo clippy --workspace --all-targets -- -D warnings
python3 crates/pu-engine/tests/agent_signals_pi_harness.py \
  --pi /isolated/install/node_modules/.bin/pi --pu target/debug/pu
```

The optional native harness requires an already installed pinned binary; it does
not install anything. It creates/removes isolated HOME, agent directory, config,
probe and socket under `/tmp`, without copying credentials. It explicitly uses
RPC only to qualify event pairing. Full native completion/allow/deny/cancel/error/
interrupt/child/resume and current-terminal/platform routes remain acceptance gates.

## Owner-qualified attention activation (Mac only)

There is an implemented activation path; no qualification or release key is enabled
by this change. After the owner records exact native notification → existing
terminal → specific pending request → response → authoritative resolution proof,
they may create `.pu/agent-signals/qualifications.json` in that host's project.
It must be a regular non-symlink file owned by the daemon user, mode 0600, at most
16KiB/32 records. The bounded reader fails closed on malformed/unknown schema,
missing proof, wrong tuple/mode/platform, expired/future grants or a 50ms read timeout.

Schema (a template only, **not an active qualification**):

```json
{
  "schema_version": 1,
  "qualifications": [{
    "tool": "pi",
    "tool_version": "1.3.0",
    "adapter_version": "1",
    "platform": "macos",
    "execution_mode": "interactive_pty",
    "owner_confirmed": true,
    "proof_record_url": "https://pagespace.ai/dashboard/DRIVE/OWNER_PROOF_PAGE",
    "qualified_at_ms": 0,
    "expires_at_ms": 0
  }]
}
```

Replace the proof URL and epoch-millisecond bounds only after owner proof; zero
bounds do not qualify. `proof_record_url` is an explicit owner attestation, not
remote verification; it is never fetched. This configuration is trusted like
other same-user project configuration, not a security boundary against malicious
same-UID agent code. No real HOME/config/auth is copied. Start a **new agent
launch** to read the record; existing launches do not silently acquire a grant.
The producer can then set `pending.actionability.macos=true` only on a validated
authoritative pending event for that exact tuple/mode. Explicit `--mode`, `--print`
or `-p` launches remain unqualified; RPC evidence cannot authorize an interactive
terminal. Snapshot projection permanently withdraws the loaded grant when it observes
expiry or time falls below the shared observation high-water (initialized at grant
load, with equal timestamps allowed); later clock rollback
cannot reactivate that launch grant; it never resolves the native request.
Intent stays intent regardless of qualification. iOS always remains false.

The native consumer separately requires its existing owner release gate:
`PP_signalQualifiedMacAdapters` contains the base64 compact JSON tuple
`["pi","1.3.0","1"]` = `WyJwaSIsIjEuMy4wIiwiMSJd`, plus fresh same-launch/session
authoritative pending facts and a unique supported terminal route. The owner's
existing native notification preferences still apply. To revoke immediately,
remove the native release gate; remove/expire the producer grant and relaunch
workers to withdraw producer capability. No new UI or APNs/service infrastructure
is involved. Gating/configuration capability alone is **not delivered native
attention notification acceptance**; real owner terminal/device proof remains open.

### Claude native observer reproduction

Build the engine lib test executable with `cargo test -p pu-engine --lib --no-run`.
Then run each scenario with its emitted `target/debug/deps/pu_engine-…` path:

```sh
python3 crates/pu-engine/verification/check-claude-signals.py \
  target/debug/deps/pu_engine-… /absolute/path/to/claude question
# Repeat with permission and error.
```

This harness creates a fresh HOME/config/project, local fake provider and real
interactive PTY. It uses dummy credentials only, drives native input, invokes the
production observer probe, checks exact pending/outcome states and exit withdrawal,
and terminates only its owned CLI/mock server. Captures remain in its printed
sandbox path for inspection. It does not build/install native apps or prove OS
notification routing, real-provider behavior, child/resume or interrupt outcomes.

### Codex native observer reproduction

```sh
python3 crates/pu-engine/verification/check-codex-signals.py \
  target/debug/deps/pu_engine-… /absolute/path/to/codex question
# Repeat with permission and error.
```

This uses an isolated real native CLI/PTY and local fake Responses provider.
It preserves `--no-daemon`, mirrors only exact captured raw PTY bytes into the real
OutputBuffer, and runs the production observer including PID-to-rollout discovery.
It asserts exact native request IDs, terminal outcomes and exit withdrawal. This
stream-mirroring test seam is not a public Engine Spawn/PTY-host or app-route proof.
No real credentials/config are copied, and no diagnostic logging is enabled.
The native OSC state is consumed in order; buffer eviction permanently loses that
launch's observation. All tool-call candidates count toward ambiguity, even unknown
tool kinds. Rollout identity/history loss is latched, with at most64 pending calls,
512 closed/superseded turn IDs,8MiB read per poll and1MiB per record. Native event IDs
come from the ordered stream offset/turn/call IDs, never receipt time. Observation
samples the existing buffer every250ms without writing input or adding a PTY reader.

### OpenCode native observer reproduction

```sh
python3 crates/pu-engine/verification/check-opencode-signals.py \
  target/debug/deps/pu_engine-… /absolute/path/to/opencode question
# Repeat with permission, error, or completion.
```

This creates fresh HOME/XDG roots and a native managed-service configuration with
an isolated loopback port. The ordinary native CLI starts its own managed service;
the production adapter only discovers and queries that service. Permission proof
checks the actual sandbox file, and question proof checks the native First answer
returned to the fake provider. Cleanup targets only that sandbox's owned CLI and
registered service. It never changes the real user's OpenCode configuration.

### Native adapter qualification tuples

The same existing private project qualification file now supports each exact
adapter tuple below. The native consumer's independent release tuple gate still
applies. Defaults remain empty. The CLI/provider harnesses above do **not** prove
notification tap → existing terminal → exact request → answer → resolution and
must not be presented as production platform attestation. iOS stays false.

| tool | tool_version | adapter_version | native Mac tuple key |
| --- | --- | --- | --- |
| claude | 2.1.300 | 2 | `WyJjbGF1ZGUiLCIyLjEuMzAwIiwiMiJd` |
| codex | 0.163.0 | 2 | `WyJjb2RleCIsIjAuMTYzLjAiLCIyIl0` |
| opencode | 2.0.28 | 1 | `WyJvcGVuY29kZSIsIjIuMC4yOCIsIjEiXQ` |

Claude acceptance-boundary checks also run `reject` and `reject_after_complete`
scenarios through the same native harness. The real CLI reports busy during a
rejecting submit hook even though it writes no accepted user turn. The adapter
requires that accepted native root record, keeps settled eligibility separate from
provisional assistant end_turn, and correlates subsequent records through the
current native parent chain. Root replay, delayed older-branch error and closed-turn
busy regressions pass. The isolated native rejection probes prove no new Working,
no invented outcome, and no revival/replacement of a previously completed event.
Parent lineage is bounded4096 IDs per turn;512 superseded roots are tombstoned,
without eviction. Exceeding a source history bound withdraws observation.

The default-environment Claude question probe also passes without
`CLAUDE_CODE_EMIT_SESSION_STATE_EVENTS`; production does not enable that flag.
Registry selection requires the exact owned PID/session/cwd/interactive tuple,
ignores foreign background rows, and rejects duplicate exact roots. Unit probes
cover a shared-PID child row, late terminal flush, root replay and bounded-history
no-ops. Native child/resume and complete public Engine qualification remain open.
