# Passive agent signal adapters

Observation is separate from process liveness. The daemon keeps its existing PTY,
input, native session, resume and shutdown paths. No adapter answers requests or
returns policy decisions. Research remains in `reports/Agent state signal discovery.md`
and `research_notes/Agent state signal discovery/`.

| Runtime | Activation | Facts and qualification limits |
| --- | --- | --- |
| Claude Code 2.1.300 | Launch-only `--settings`; exact preassigned native session; skip if user already supplied `--settings` | Submit is source-confirmed entry only and cannot establish Working or clear an outcome. StopFailure is an API-failure boundary, authoritative only with its own native prompt/turn identity and a matching independently confirmed active turn. No authoritative Claude activity source is enabled; hook-only evidence therefore remains Unknown. Stop and permission/question/MCP intent do not establish completion or displayed pending input. Actual isolated print-JSON SessionStart/UserPromptSubmit captured with exact session via this CLI relay; other native scenarios not run. |
| pi 1.3.0 | Launch-only `--extension`; first private frame must match owned PTY PID before exact native session binding | Native agent_start, final settled outcome, UI prompt pairs. Generic confirm means question, never permission. RPC confirm/select/input pairs observed with this extension/helper, no model credentials; model lifecycle/terminal notification routes not qualified. Native session changes within a process are not silently rebound. |
| OpenCode v2.0.28 | Read-only authenticated queries of the existing managed service; exact launch-assigned root session | Discovers only its private owner-mode0600 XDG state registration. Validates loopback URL, exact service version/PID, session ID, root parent and directory; GET-only active/session/permission/form snapshots project Working, terminal outcomes and pending requests. Checks execution state around the snapshot to reject mixed transitions. No plugin injection, service start/restart, --standalone, ownership migration or user configuration changes. Native registration/session/form payloads captured with real2.0.28 in an isolated HOME; complete model-turn and native terminal interaction qualification is still in progress. Explicit custom --server/--standalone routes are not yet observed. |
| Gemini 0.64.0 | Fixture-only normalizer | Pin 4a0749aceac5643e6f85a36d7420314d85e13b52. Start/end boundaries and ToolPermission subtype intents; no proved launch-only settings/exact root session installation or universal resolution/error hooks. |
| Codex CLI 0.163.0 | Fixture-only hook normalizer | Source pin 5009a99dfc07830ddf7464c26f310a2b8532b55d is not binary equivalence proof. No app-server ownership; no prose question/error inference. |
| Cursor | Fixture-only stop-boundary normalizer | CLI parity and install/timeout unknown; no ACP ownership, no generic IDE hooks presented as CLI proof. |

Every attention actionability flag remains false. RPC UI proof does not establish
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
  The proposed universal 100ms wall-clock hook budget is **not proved**. Independent review also observed a first cold helper test exceeding 500ms (warm rerun passed); this remains a recorded qualification failure. Native
  Claude registration also sets a one-second hook timeout.
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
