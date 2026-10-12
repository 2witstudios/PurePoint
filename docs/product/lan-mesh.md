# LAN Mesh

**Maturity: SPECIFIED** | ID Prefix: MESH | Dependencies: `product/daemon.md`, `product/output-streaming.md`, `architecture/ipc-api.md`, `architecture/agent-execution.md`

## Purpose

Run and control agents on any enrolled machine on the local network from one host Point Guard. A worker is any machine running `pu-engine` with configured agent runtimes; it does not need its own Point Guard. The worker stays authoritative over its filesystem, projects, runtimes, processes and capacity. The host requests work against receiver-issued export identities and observes and controls the resulting runs.

Plan of record: PageSpace `c93of1sevwu4kquan9qki721` (plan) and `raw6l2jwxma2b91v2sv48bbv` (execution record).

## Conceptual Model

```
Host daemon (Point Guard machine)                Worker daemon (any LAN machine)
  MeshRuntime                                      MeshRuntime
    HostClient: one pinned link per worker   ==>     paired listener (control + bulk)
      control: requests, heartbeats                   SessionAuthority (authority barrier)
      bulk: demultiplexed output watches               Executor: per-agent boundary
    worker routes (fingerprint -> addresses)           Ledger (SQLite): peers, windows,
  local IPC: pu mesh ...                                  agents, runs, writers, inputs
                                                       MeshExports: receiver-issued exports
                                                       Guardian per run (process-tree lifetime)
```

Four independent authorities:

- **Identity:** an Ed25519/ECDSA self-signed certificate per daemon. Its SHA-256 fingerprint is the peer identity. TLS 1.3 mutual authentication pins the exact certificate in both directions; addresses never authenticate.
- **Authorization:** the receiver's own grants per peer. A grant names an export, explicit capabilities (`inspect`, `read_output`, `spawn`, `control`, `input`, `stop`, `suspend`, `resume`) and runtime IDs. There are no wildcards and no onward delegation.
- **Execution ownership:** the worker's durable ledger admits each launch once per operation identity. It owns the agent/run identity and reserves capacity until supervisor proof releases it.
- **Per-agent control:** a controller principal with a monotonically increasing epoch and revision, and one input writer per run and epoch.

## Decisions

### ! [MESH-001] Trust model: trusted personal fleet, zero-trust network

The v1 fleet is one owner's machines, and the agent runtimes are trusted programs. The network and every peer are untrusted until proven:

- Mutual TLS 1.3 pins the exact certificates.
- Enrollment happens out of band with an independently verified fingerprint.
- Every request is re-authorized against the receiver's current grants under the authority barrier.
- There is no remote administration: identity, enrollment, grants, exports and listener configuration are only reachable over owner-UID local IPC.
- Nothing is delegated onward.

Same-account processes on a worker are inside the trust boundary, as with the existing local daemon.

### ! [MESH-002] Worker run lifetime: per-run guardian

macOS has no supported unprivileged mechanism that terminates arbitrary detached descendants when the daemon dies. launchd cleanup is scoped to a process group, kqueue `NOTE_TRACK` is `ENOTSUP`, and coalitions require privilege. Each remote run therefore executes under a guardian (`pu-engine --mesh-guardian`), which:

- is the PTY session leader;
- forks the runtime;
- marks the run tree with a secret token in the environment (never on argv) and an inherited lease descriptor;
- tracks descendants by ancestry, marker and `(pid, start time)`.

The guardian terminates the whole tree when the daemon dies (kqueue `NOTE_EXIT` / `PR_SET_PDEATHSIG`), when the agent exits, or when it is signalled. It does this with SIGTERM, a grace period, SIGKILL and re-scanning, then writes a verified outcome record into a private directory. On Linux it is also a child subreaper.

Only a verified outcome releases the run's capacity, as `ProcessTreeTerminated`. Anything less leaves the run `Unknown` and reserved until explicit owner disposition.

**Residual limit:** under this trusted-fleet model, the contract does not cover a descendant that scrubs its environment, closes the lease and leaves the tree within one 250 ms scan before it is observed.

The daemon spawns the guardian with SIGTERM, SIGHUP and SIGINT blocked across exec, so a stop during guardian startup is queued rather than fatal. Graceful daemon shutdown only asks guardians to terminate and never SIGKILLs them.

### ! [MESH-003] Hub-and-spoke topology with one link per worker

One host directs the workers. A worker allows one live control binding per peer: a new binding fences the previous one and its bulk connection. The host therefore keeps exactly one link (control plus independently authenticated bulk) per worker, carries all requests on it, and demultiplexes output watches from the shared bulk.

Worker-to-worker routing, relay, public traversal, consensus and automatic failover are out of scope.

### ! [MESH-004] Explicit listener configuration and routing

The default is disabled: no listener exists until the owner initializes an identity and configures specific same-IP control and bulk addresses. Wildcard, multicast and broadcast addresses are refused. A worker with no active enrolled peer opens no listener.

Every administrative mutation stops and drains the old listener before it publishes, and the applied state (`pu mesh listener`) is reported separately from persisted intent. Host routes (fingerprint, daemon ID, addresses) are hints stored in the host's private config record, and only name already-pinned peers.

### ! [MESH-005] Exact source, no fallback

A spawn names an export, a granted runtime, a full commit ID and a prompt. Before admission the worker verifies that the exact commit exists in the export's repository. It then creates a receiver-derived worktree (`wt-mesh-<digest>`, branch `pu/mesh-<id>`) at that commit and runs its own configured command for the runtime.

There is no branch, commit or machine fallback. Callers never supply executables, arguments, paths or shells. The plain terminal runtime is not offered.

### ! [MESH-006] Remote operations offered in v1

Offered operations:

- inventory and status, which include the controller-only writer checkpoint;
- operation windows;
- spawn, acquire control and stop;
- opening a writer, input, input and operation receipts;
- resize;
- subscribing to and unsubscribing from output.

Remote suspend, resume and native transcript reads return `unsupported`. A host continues work by spawning a new run, and observes work through the output stream.

### ! [MESH-007] Local owner precedence

Local kill, suspend, resume, rename and worktree deletion never act on a worker-owned run. Local input and resize are allowed only after `pu mesh run takeover`, which fences the remote controller's sessions and writer under that peer's exclusive barrier. A peer cannot displace the local controller. `pu mesh run stop` uses the same durable stop and guardian proof.

`pu mesh run resolve` records an owner attestation, but only for an `Unknown` run with no live process. Verified guardian proof is always preferred.

### ! [MESH-008] YAML parsing in safe Rust

`unsafe-libyaml` (RUSTSEC-2024-0449, unmaintained) has been replaced by `serde-saphyr`, with characterization tests pinning existing configuration semantics (see `crates/pu-core/src/yaml`).

### ! [MESH-009] Ledger scope: remote runs, not local spawns

Every remote spawn and control, and every local action on a remote-owned run, goes through the worker ledger and the per-agent boundary. Local `pu spawn` runs are not admitted into mesh capacity. Admitting them would require running every local agent under the guardian, because without guardian proof their reservations could never be released. That would change local behavior today: descendants such as dev servers would end when the agent exits.

Local runs therefore do not consume or contend for remote capacity, and remote-owned runs can never escape the ledger through local commands. Revisit this if local agents adopt the guardian contract.

## Requirements

- Given an unknown, revoked, expired or mismatched peer or certificate, should reject at authentication or authorization without revealing inventory, and without any downgrade or silent identity reset.
- Given trust administration, revocation or a peer generation change, should persist authority first, then fence every affected live control and bulk session; a revoked last peer closes the listener.
- Given a spawn, should verify the exact source commit before any reservation, then atomically admit and persist the operation, agent/run identity and intent before process creation, and return durable worker/agent/run identity.
- Given a duplicate operation, should return its retained receipt without launching or reserving again; a conflicting payload under the same identity should return `conflict`.
- Given missing source, an invalid or revoked export, an ungranted runtime, an offline worker or an incompatible version, should fail explicitly with no fallback.
- Given takeover, revocation or a peer generation change racing a side effect, should serialize at the authority barrier, then the per-agent boundary, then the ledger transaction, with no check-then-write gap.
- Given input, should accept each sequence once, make one bounded write attempt and report `written`, `partial{bytes}`, `not_written` or `unknown`. A lost acknowledgement or a writer replacement should never cause replay, and a replacement writer must name the inspected predecessor checkpoint.
- Given one current controller and writer per run and epoch, should fence the previous writer on takeover or replacement, and should fence local writers while a peer controls the run.
- Given output saturation, a stalled bulk reader or slow viewers, should keep control on its own connection, bound memory to the run's ring and frame queues, report evicted bytes as explicit gaps, and leave worker execution unaffected.
- Given worker daemon death, should terminate the run tree through the guardian and, on restart, resolve the original run from verified proof (`interrupted`). Runs without proof stay `unknown` and reserved.
- Given host death or disconnection alone, should leave worker-owned runs and reservations intact. The reconnecting host rediscovers the generation, inspects receipts and explicitly resumes control of the original run.
- Given concurrent admissions from several hosts, should admit each operation exactly once within capacity limits.

## Sum Sheet

```
spawn(op) ⇒ verified(commit) ∧ granted(peer, export, runtime) ∧ admitted_once(op) ∧ guardian(run)
release(run) ⇔ verified_outcome(guardian, run) ∨ owner_attests(run ∧ ¬live)
input(seq) ⇒ fresh(seq) → one_attempt ∧ precise_outcome ; ¬fresh(seq) → retained_receipt
controller(run) = local ⇒ ¬peer_input ∧ ¬peer_displace
host_down ⇒ run continues ; worker_down ⇒ tree terminated ∧ run resolved after restart
```

## Interfaces

**CLI (owner-local):**

- `pu mesh identity initialize|show|export`
- `pu mesh enroll-manual`, `pu mesh policy-update`, `pu mesh peers`
- `pu mesh peer revoke|regrant|advance-generation`
- `pu mesh export register|list|revoke`
- `pu mesh config show|disable|enable --control-address A --bulk-address B`
- `pu mesh listener`
- `pu mesh worker add|remove|list`
- `pu mesh run list|takeover|release|stop|resolve`

**CLI (host to worker):** `pu mesh remote --worker <fingerprint|label> inventory|status|window|spawn|acquire|stop|writer|input|input-receipt|resize|watch`. Mutations print their `--window/--operation` identity so the same command can be retried safely.

**Local IPC:** `MeshAdmin`, `MeshListenerStatus`, `MeshRoute`, and `MeshWatch` (a stream of `MeshOutput` frames). See `architecture/ipc-api.md`.

**Bridge wire** (`pu_core::mesh::wire`, version 1): `Hello::{Discover,Control,Bulk}`, `Welcome`, `GenerationDiscovery`, `Request{request_id,target_daemon,operation}`; `response::{Response,Body,OutputFrame}`. Control frames are capped at 64 KiB, bulk frames at 256 KiB, raw output chunks at 32 KiB, and input at 8 KiB.

**Defaults:**

- Control sessions: 8 total, 2 per peer.
- Bulk sessions: 8 total, 2 per peer.
- Subscriptions: 16 total, 4 per peer.
- Session deadlines: 250 ms send, 5 s read.
- Host link heartbeat: 2 s.
- Host budgets: 16 concurrent requests and 64 watches.
- Ledger capacity: 256 global runs, 64 per project, 128 per runtime, 64 per principal, 16 starting.
- Pre-authentication: 32 TLS handshakes in flight, 4 per source address (IPv6 per /64), 1 s handshake deadline. Session permits are reserved only after the pinned certificate check.
- Ledger budgets count only live records. Each peer holds at most 16 open windows, 256 live operations, 64 live writers and 512 uncertain input receipts. The host reuses one window per worker generation for half its lifetime. The local owner has a reserve of 64 windows and 512 operations that peers cannot consume. The newest 256 terminal runs are retained; older history is compacted automatically.
- Prompts: at most 32 KiB, never starting with `-`, no control characters other than newline and tab.

## Edge Cases

- **Worker restart:**
  - The ledger reopen closes operation windows, advances peer generations, clears peer controllers, marks live runs `unknown` and fences writers.
  - The host's first request on its stale link fails, and the next request rediscovers and reconnects; the host never retries automatically.
  - Retained receipts remain readable as evidence.
- **Stop during guardian startup:** queued by the blocked signal mask, then handled with proof.
- **Concurrent export resolution:** catalog transactions wait up to 2 s for one another instead of failing.
- **Concurrent worktree creation on one repository:** serialized per repository for both local and remote spawns.
- **Disabling while viewers are attached:** the listener is stopped and drained before the reply. Host watches get `closed` and may resubscribe from their cursor; the worker reports any gap.
- **Owner resolution of a run whose guardian left no record:** only for `unknown` runs with no live process; the attestation text is stored as evidence.

- **Retained receipts:** a retried operation returns its receipt while its window is open or its run still holds capacity. After that it is refused as `receipt_expired`.
- **Control ownership:** only the principal that spawned a run can acquire control of it.
- **Residual limits:**
  - A same-account process could pre-write a forged guardian outcome record (consistent with MESH-001).
  - Several unauthenticated hosts working together can slow, but not lock out, an enrolled peer.
  - Once compaction removes a terminal mesh agent from the ledger, its local manifest entry is no longer fenced.
  - A stop signal targets the guardian's process group after confirming the session has not exited; between the kernel reaping the guardian and the exit being observed (microseconds) the group ID could in theory be reused.
  - Each guardian scans the owner's processes every 250 ms (process table, plus environment and descriptors of newly started processes), so the cost grows with active runs × processes.

## Research Notes

Evidence is in the execution record and in these suites:

- `crates/pu-engine/tests/mesh_proof.rs`: real daemon processes. Covers worker SIGKILL with tree termination and reconciliation, host SIGKILL followed by reconnect and control without replay, revocation mid-run, and concurrent admission from two hosts.
- `tests/mesh_exec.rs`: two daemons in one process. Covers spawn, duplicates, conflicts, exact input outcomes, watch, stop with guardian proof, the local fence, takeover, and local stop.
- `tests/mesh_guardian.rs`: the guardian against real processes.
- `tests/mesh_load.rs`: the fleet workload, with results below.
- The `mesh_*` unit suites.

Platform evidence: macOS 26 (Apple silicon) runs all suites locally. Linux evidence comes from the ubuntu-latest CI jobs, which run the same suites, including the guardian's subreaper path. Each platform claim rests only on its own runs.

### Fleet load (8 workers × 30 agents)

Measured on 2026-10-12 with `cargo test --release -p pu-engine --test mesh_load -- --ignored` on macOS 26 (Apple silicon, one machine over loopback). The run used 9 real daemon processes and 240 guarded agents (480 processes, 240 PTYs), 16 live host viewers, one run flooding output, and one peer subscribed to it that never reads its bulk connection.

| Measure | Result |
|---|---|
| Spawn request (240) | p50 357 ms, p95 895 ms, max 1.17 s (includes exact-source check and worktree creation) |
| All 240 running | 19.4 s after first spawn |
| Control `Status` round trip, all workers under load (320) | p50 16 ms, p95 174 ms, p99 288 ms, max 316 ms |
| Same, on the worker with the stalled bulk reader (40) | p50 12 ms, p95 87 ms, max 98 ms |
| Live viewers during the stall | 16 / 16 |
| Host daemon | 15 MiB RSS, 50 fds |
| Worker daemon | 21–30 MiB RSS, 52–53 fds |
| Teardown (stop all 240 with guardian proof) | 5.4 s, every run tree ended |

A debug build of the same workload measured a control p95 of 538 ms. These numbers are tested capacity for one machine over loopback; real-LAN latency adds network round trips. The macOS limit `kern.tty.ptmx_max` (511 by default) bounds PTYs per worker.
