# Passive agent signal adapters

Observation is separate from process liveness. The daemon keeps its existing PTY,
input, native session, resume and shutdown paths. No adapter answers requests or
returns policy decisions. Research remains in `reports/Agent state signal discovery.md`
and `research_notes/Agent state signal discovery/`.

| Runtime | Activation | Facts and qualification limits |
| --- | --- | --- |
| Claude Code 2.1.300 | Read-only native `sessions/<owned-pid>.json` registry plus incremental exact root transcript; no polling subprocesses or injected hooks/settings | Exact owned PID/session/cwd/interactive kind/start epoch, exact CLI/transcript version. Native busy means Working only for an accepted root user turn that has not settled; native waiting category is joined to a single unresolved native tool-use ID for Question or Permission. Native idle plus structural assistant end_turn/API-error metadata on the current parent chain means Completed/Errored. Out-of-order native parent edges retain only bounded IDs/projected facts until the accepted root is proven; retired branches cannot affect a later root. Rejected submit attempts cannot create/revive Working; delayed older-branch output cannot close a newer turn. Ambiguous/missing IDs, future versions, child/foreign records and incomplete/mixed views withdraw observation. Real interactive CLI plus local fake Anthropic provider passed question→answer→completion, manual Write approval→completion, and API400→error through the production observer; no real credentials or config copied. Public Engine Spawn→Working→Question/Permission→raw Input→Completed and API error→Errored passed, including real bridge and Mac/phone model decoding. Native cancellation withdraws Working without inventing a terminal result; a subsequent accepted prompt was verified to complete. Native permission denial/question dismissal and subsequent-turn recovery, public suspend/resume with native session preserved/new launch identity, and real git worktree launch also passed. Concurrent dialogs, MCP elicitation and native-app route qualification remain open. |
| pi 1.3.0 | Launch-only `--extension`; first private frame must match owned PTY PID before exact native session binding | Native agent_start, final settled outcome, UI prompt pairs. Generic confirm means question, never permission. RPC confirm/select/input pairs observed with this extension/helper, no model credentials; model lifecycle/terminal notification routes not qualified. Native session changes within a process are not silently rebound. |
| OpenCode v2.0.28 | Read-only authenticated queries of the existing managed service; exact launch-assigned root session | Private owner-mode0600 XDG state registration, loopback URL, exact service version/PID, root ID and directory. GET-only active/session/permission/form snapshots project Working, native terminal outcomes and pending request IDs; execution/session views are checked around the query. No plugin injection, service start/restart, --standalone, ownership migration or user configuration changes. Production observer passed real interactive Write approval→actual sandbox file→completion, question→native First answer→completion, and API400→error with a local fake provider. Harness services use distinct disposable config ports. Public Engine Spawn→Working→Question/Permission→raw Input→Completed and API error→Errored passed, including real bridge and Mac/phone model decoding. Native double-Escape interruption projects Interrupted. Native permission rejection (write absent), question dismissal and subsequent-turn recovery, public suspend/resume and real git worktree launch also passed. Native child-session and native-app route qualification remain open. Explicit custom --server/--standalone routes are not yet observed. |
| Gemini 0.64.0 | Fixture-only normalizer | Pin 4a0749aceac5643e6f85a36d7420314d85e13b52. Start/end boundaries and ToolPermission subtype intents; no proved launch-only settings/exact root session installation or universal resolution/error hooks. |
| Codex CLI 0.163.0 | Existing PTY OSC7501 plus incremental rollout held by the exact owned native PID; no hook/config/ownership changes | Actual native idle/working/blocked packets, exact CLI root/session/cwd/version and native turn/call IDs. Working requires an accepted native task_started record; definitive typed terminal outcomes remain settled despite later auxiliary UI activity. Generic blocked is classified only with one outstanding known root call; unknown or concurrent calls fail closed. Native task_complete.error distinguishes API failure from completion. Native causal turn metadata prevents delayed calls/results with reused call IDs from changing a newer turn. Explicit root terminal records resolve requests; source truncation/replacement/byte loss invalidates observation. Production observer passed real isolated interactive permission→approval→completion, question→answer→completion, and API400→error using a local fake provider and a disclosed raw-PTY-to-OutputBuffer test seam. A spoofed OSC packet in assistant content was stripped by the native UI. Public Engine Spawn→Working→Question/Permission→raw Input→Completed and API error→Errored passed, including real bridge and Mac/phone model decoding without the test seam. Native Escape interruption projects Interrupted. Native permission denial/question dismissal and subsequent-turn recovery, public suspend/resume and real git worktree launch also passed. Native child-session and native-app routes still require qualification. |
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
  Native HTTP requests disable proxies/redirects, cap each response at 1MiB,
  have a 1s request timeout and 2s overall snapshot deadline, and poll at 500ms.
  A failed snapshot withdraws observation; a subsequent complete native snapshot
  can reconcile it. Owned PTY exit irreversibly withdraws that launch's observer.
  Each snapshot holds at most 64 pending IDs and refers only to the assigned root.

* Claude observation runs outside the PTY. It reads only that owned PID's registry record and exact root transcript; it never launches list subprocesses or scans all agents per poll.
  A native poll has a 5s overall deadline. Registry reads reject symlinks, nonregular/foreign-owned or group/world-writable files and data over 16KiB; exact PID/domain/session/cwd/kind/entrypoint/protocol/version/start epoch are required. Native registry and transcript versions are checked directly; no helper CLI process is launched. Registry start time must postdate the recorded pre-spawn lower bound, so a crashed predecessor's record cannot bind a reused PID.
  Transcript reads are incremental, at most 8MiB per poll/32MiB per record, and retain
  only IDs, tool categories and structural outcomes. Existing histories larger than
  the per-poll budget are caught up incrementally. A timed-out Claude poll retains
  its single in-flight reader and private replay/settlement state. Native registry is sampled on
  both sides, and transcript inode/length must still match afterward. Truncation,
  replacement or invalid root history latches observation unavailable for the launch.
  Up to 64 unresolved tool IDs are retained. Queries do not change user settings,
  input, resume, process/session ownership or permission policy. No hook/helper is
  needed for Claude. Initial idle without a terminal result does not invent Finished.

* Claude/Codex assemble records across polls up to 32MiB, so ordinary native
  image/PDF payloads do not exceed a text-only 1MiB limit and permanently disable
  observation. The 8MiB per-poll I/O budget remains unchanged; only structural
  facts/IDs survive parsing. Larger records still fail closed at the explicit cap.

Native observers publish semantic changes immediately. Timestamp-only refreshes
are coalesced into a 10-second heartbeat, below the consumer's 30-second lease;
qualification withdrawal and disconnection still publish immediately. Codex source discovery is shared within the existing Engine: one process-tree
snapshot and one PID-tagged, owned-descriptor query cover up to 256 observed
launches every 2 seconds. Native Working edges request a newer view with shared
single-flight coalescing and a 250ms global retry floor. At most 128 owned PIDs
per root, 4096 PIDs per batch and 8MiB of lsof output are accepted; overflow fails
closed. Roots in the same directory cannot borrow each other's descriptors.
The existing startup native-read/session binder uses this sampler too: it shares
the observer's exact PID/cwd launch registration, rather than repeating its old
100ms ps/lsof loop. Its existing guarded manifest commit and ambiguous-source
failure are preserved. Without telemetry support it registers its own bounded
reader; exited owned handles stop unbound discovery. Read binding remains owned
by the existing engine path, never by the passive telemetry observer.
Bound Codex launches recheck that the owned process still holds the same root,
not merely that the old file still exists. A different root latches observation
lost without rebinding the native session; a temporarily unavailable check
withdraws observation until the original root is proved again.

OpenCode observers of the same private service registration share a 500ms
sampling group: two health reads and two active-map reads per batch, plus four
root-specific reads per agent (4 + 4N requests, rather than 8N). Each root's
metadata is checked before and after the shared active-map bracket; unrelated
root changes do not invalidate a stable root. Root reads have at most 32 concurrent
requests, with existing per-request limits and a 2-second whole-batch deadline.
A service group accepts at most 256 launch registrations. Failures are cached at
the same bounded cadence; cached native timestamps never renew source freshness.
Registration guards remove finished launches, weak service entries are reclaimed,
and registration epochs prevent cache reuse after replacement. Samplers belong to
the Engine, not a global singleton or new service; they do not start processes,
change PTYs, write native config or migrate native session ownership.

Codex checks the executable's exact version once before launch with a 500ms
deadline and bounded output, using a disposable HOME/CODEX_HOME because native
`--version` can create PATH aliases. No user profile is passed to that check.
On resume, the exact root file's inode and complete-line EOF are captured before
the new process starts (500ms preparation deadline). Only subsequently appended
records can establish that launch's work or requests; the new owned PID must
independently hold the same root file. This prevents an interrupted historical
turn from appearing Working while the resumed terminal is idle. An unavailable,
partial or replaced source disables observation without preventing native resume.

Claude resume uses the same pre-spawn inode/complete-line EOF boundary, with no
accepted turn, pending requests, replay history or terminal outcome inherited.
This also avoids exhausting the new launch's bounded history on old sessions.
OpenCode resume samples its existing service before spawn (500ms preparation
deadline) and captures the native `time.idle` terminal revision. Native v2.0.28
advances this revision strictly, even for same-millisecond terminals and clock
rollback. Unchanged prior outcomes are suppressed; a new active execution raises
the minimum terminal revision, so its predecessor cannot reappear when active
clears. If the prelaunch service view is unavailable, the first complete native
view establishes the baseline rather than claiming an old outcome as new work.
Activity and pending requests are different: the managed service reports current
execution and live pending conditions of that same native session. A resumed TUI
can observe work still executing in that service without submitting a new prompt.
Do not hide real current work/requests or restart the service to force a new epoch.

## Checks and reproduction

```
cargo test -p pu-engine --lib signals::
cargo test -p pu-cli --test signal_helper
cargo clippy --workspace --all-targets -- -D warnings
python3 crates/pu-engine/tests/agent_signals_pi_harness.py \
  --pi /isolated/install/node_modules/.bin/pi --pu target/debug/pu
```

For the public Engine route, compile a standalone runner from exact Cargo artifact
paths, then select one native tool and scenario (question, permission, error, interrupt, permission_deny, question_cancel, resume, worktree or question_hold; Codex also supports resume_pending, Claude/Codex support image):

```sh
python3 crates/pu-engine/verification/build-engine-signals-probe.py /tmp/pp-signals-proof
python3 crates/pu-engine/verification/check-engine-signals.py codex question \
  /absolute/path/to/codex --runner /tmp/pp-signals-proof/owned-native-signals
```

`question_hold` retains an actual question for 35 seconds and verifies bounded
subscription heartbeats keep the last delivered snapshot current before answering.
The provenance guard includes tracked and untracked source/workspace inputs, and
refuses a build whose source changes during compilation. Resume has a 200-second
wrapper budget; cleanup escalates a stuck runner without skipping native-service
and fixture-server teardown. Codex `resume_pending` suspends at an actual pending
permission, checks the resumed idle state before new input, then verifies a fresh
turn completes without inheriting the previous process's work or request.
The `image` case reads a real noisy PNG through native Read/view_image, requires
actual image content in native JSONL and completion through the public Engine;
Claude additionally requires a native record larger than the former 1MiB limit.
Quiescent resume cases check idle/Unknown with no inherited outcome before new input;
`resume_error` checks a prior API error followed by resume and a successful turn.
Codex `session_switch` exercises actual `/new`, requires a distinct native root
and verifies old facts are unavailable without changing PurePoint session ownership.
OpenCode `resume_live_work` is the explicit exception to idle-resume setup: the
native service continues a real model request after its old TUI exits. The local
provider holds that request, the resumed TUI/source must show Working without new
input, and releasing the provider must produce Finished. Static past outcomes
remain suppressed; live session conditions are freshly observed, not replayed.

Use `claude` or `opencode` with its pinned absolute binary path for the other
adapters. The driver verifies runner provenance, uses fresh private HOME/config
and a local fake provider, then invokes public Engine Spawn/Input/Status/Inventory
and captures status events. OpenCode waits for its actual input UI before typing.
It retains captures under the printed private `/tmp/ppqa-*` directory, kills the
exact owned agent, and stops only its isolated native OpenCode service. No app or
system daemon is installed. This root-session proof does not cover child sessions,
OS notification taps, or a real provider's model behavior.

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
tool kinds. Rollout identity/history loss is latched, with at most 64 pending calls,
512 closed/superseded turn IDs, 8MiB read per poll and 32MiB per assembled record. Native event IDs
come from the ordered stream offset/turn/call IDs, never receipt time. Observation
samples the existing buffer every 250ms without writing input or adding a PTY reader.

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
Parent lineage is bounded 4096 IDs per turn; 512 superseded roots are tombstoned,
without eviction. Exceeding a source history bound withdraws observation.

The default-environment Claude question probe also passes without
`CLAUDE_CODE_EMIT_SESSION_STATE_EVENTS`; production does not enable that flag.
Registry selection requires the exact owned PID/session/cwd/interactive tuple,
ignores foreign background rows, and rejects duplicate exact roots. Unit probes
cover a shared-PID child row, late terminal flush, root replay and bounded-history
no-ops. Native child/resume and complete public Engine qualification remain open.
