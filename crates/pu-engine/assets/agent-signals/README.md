# Passive agent signal adapters

Observation is separate from process liveness. The daemon keeps its existing PTY,
input, native session, resume and shutdown paths. No adapter answers requests or
returns policy decisions. Research remains in `reports/Agent state signal discovery.md`
and `research_notes/Agent state signal discovery/`.

| Runtime | Activation | Facts and qualification limits |
| --- | --- | --- |
| Claude Code 2.1.300 | Launch-only `--settings`; exact preassigned native session; skip if user already supplied `--settings` | Submit is source-confirmed entry only and cannot establish Working or clear an outcome. StopFailure is an API-failure boundary, authoritative only with its own native prompt/turn identity and a matching independently confirmed active turn. No authoritative Claude activity source is enabled; hook-only evidence therefore remains Unknown. Stop and permission/question/MCP intent do not establish completion or displayed pending input. Actual isolated print-JSON SessionStart/UserPromptSubmit captured with exact session via this CLI relay; other native scenarios not run. |
| pi 1.3.0 | Launch-only `--extension`; first private frame must match owned PTY PID before exact native session binding | Native agent_start, final settled outcome, UI prompt pairs. Generic confirm means question, never permission. RPC confirm/select/input pairs observed with this extension/helper, no model credentials; model lifecycle/terminal notification routes not qualified. Native session changes within a process are not silently rebound. |
| OpenCode v2.0.28 | **Unavailable in production**; passive plugin artifact/normalizer fixtures only | Pin 026b106bc5d41b679ffc413264229c340871e4d3. `packages/cli/src/services/server-connection.ts` reuses managed service; `server-process.ts` consumes OPENCODE_CONFIG_CONTENT only at server startup. Existing launch exposes no proved authenticated endpoint/root association for per-launch intake. No --standalone, server restart, global config mutation or private server added. Plugin setup uses existing `context.event.subscribe`; forms need not block work. |
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
