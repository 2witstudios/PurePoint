# Authenticated session boundary

This is an opt-in library composed with the real `MeshStore`, `Ledger`,
`MeshTransport` and `owned_work`. It creates no listener, proxies no local IPC,
starts no agent/PTY, and enables no runtime bridge. Stable identity/enrollment
must be explicitly prepared locally before constructing it.

`SessionAuthority::new(store, ledger, limits)` consumes both stores and owns one
Tokio authority RwLock. The parent must route **all** subsequent store/ledger
administration through `administer` or its `revoke`/`update_policy` wrappers.
`AuthorityState` is the place to add the parent's export store; extend borrowed
closure arguments when adding exact export/run lookup. Never retain a returned
policy/caller as authorization for a later effect.

## API and operation ownership

Dispatch, admin and output-producer hooks are public for trusted Rust embedding.
They do not add any network operation or bypass authenticated session, LocalAdmin,
WorkOwner or retained-barrier requirements. Embedders obtain WorkOwner from the
local IPC server; the parent still supplies the exact resource/dispatch boundary.

- `accept(transport, io, Role)` reserves a role permit before TLS, then a per-peer
  permit using the actual certificate-derived identity. Build `transport` from
  the consumed store's identity and explicitly enrolled pins before composition.
- Control Hello must name the exact daemon, supported bridge version and current
  receiver-owned peer generation. Welcome contains fresh UUIDv4 incarnation and
  control-binding IDs. A fresh control replaces that peer's previous binding.
- Bulk uses independent mTLS/ALPN and must match the live control's certificate
  peer, daemon, incarnation, generation and binding. Knowing the binding string
  supplies no TLS identity. Closing/replacing control wakes associated bulk.
- `dispatch(work, effect)` reads and validates a restricted typed bridge request,
  then performs canonical policy/ledger checks and the effect on `spawn_blocking`.
  `effect(&Ledger, &Caller, &PeerPolicy, &Request) -> Result<Vec<u8>>` runs under
  shared authority; a fresh check precedes the bounded response write, under the
  same retained guard. The parent must continuously drive control dispatch reads
  to observe TLS EOF and idle deadlines. There is no detached authorized snapshot.
- `subscribe(export)` allocates a bounded resource lease, not cached authority.
  `send_output_with(subscription, producer)` is the parent integration path:
  `producer(&Ledger, &Caller) -> Result<(raw_byte_count, encoded_frame)>` runs
  under shared authority, allowing exact receiver resource checks and bounded
  output copy inside the guard. Live policy is checked before and after copying,
  and the guard remains held through the bounded TLS write. `send_output` is a
  convenience for already prepared frames, with fresh send authorization.
- `administer(work, admin, peer, change)` retains owned exclusive authority inside
  the actual blocking closure, then wakes/closes the peer's binding even on a
  failed change. `revoke` uses the real durable ledger fence. It does not terminate
  worker runs or release reservations. Admin and dispatch use `WorkOwner`: aborting
  a transport waiter cannot abort accepted work. Blocking owners retain their
  authority guards and admission leases until actual completion. Handshake
  validation and observation also retain their leases in their blocking closures.
- Framing errors/cancellation poison the transport. Session never returns a
  partially consumed connection for reuse. Idle network reads hold no authority;
  registry mutexes never surround an await or acquisition of authority. Where both
  are needed the order is authority, then registry. There are no application queues.
- `ControlClient` validates typed Hello/Welcome and receiver identity/target/
  generation, validates typed Request before serialization, and decodes responses
  into the caller's response type. `connect_bulk` proves its independent mTLS peer
  and exact Welcome binding. `BulkClient::read` is bounded and typed.

InputReceipt uses current `Inspect` authorization, as lifecycle Receipt does;
removing Control/Input while keeping Inspect permits deliberate reconciliation.
The parent must verify exact writer/agent/run/export and stable principal ownership
before returning an acknowledgement (including old writer/current generation).
Inspection never replays bytes or reopens a writer.

## Default budgets

| Budget | Default | Configuration ceiling |
|---|---:|---:|
| Control connections total / per peer | 8 / 2 | total 64, peer <= total |
| Bulk connections total / per peer | 8 / 2 | total 64, peer <= total |
| Subscriptions total / per peer | 16 / 4 | total 256, peer <= total |
| Encoded control frame | 64 KiB | wire 64 KiB |
| Encoded bulk frame | 256 KiB | wire 256 KiB |
| Raw output chunk | 32 KiB | 32 KiB |
| Whole send deadline | 250 ms | 1 second |
| Whole idle frame read deadline | 5 seconds | 30 seconds |

Configuration rejects zero/invalid/inconsistent limits. TLS negotiation also has
MeshTransport's fixed five-second deadline. Transport allocation is always
bounded by its wire ceiling; a configured smaller frame limit is additionally
checked after transport decode. Producer/dispatcher implementations must bound
allocations before returning their frames. Connection permits bound concurrent
validation/effect chains, including cancelled waiters. Closed session/subscription
objects release their permits on close/drop; retained accepted operations keep
leases until completion.

## Scratch evidence (2026-10-11)

Environment: Apple M4, aarch64, macOS Darwin 25.6.0. Cargo artifacts are isolated
at `/tmp/purepoint-mesh-session-target`; actual temporary private directories and
SQLite databases are created per test. TLS runs over bounded duplex (4 KiB) and
real loopback TCP. TCP uses small kernel receive/send buffers and a deliberately
stopped bulk reader; a finite maximum of sixteen 256 KiB writes must hit the
bounded send deadline.

Command:

```sh
CARGO_TARGET_DIR=/tmp/purepoint-mesh-session-target cargo test -p pu-engine --lib mesh_sessions -- --nocapture
```

17 session tests pass. The final combined mesh check (`cargo test -p pu-engine --lib mesh_ -- --nocapture`) passes 47 session/transport/store tests. Production `cargo check -p pu-engine --lib` also passes. Strict all-target Clippy (`cargo clippy -p pu-engine --all-targets -- -D warnings`) passes; the documented callable hooks are public for trusted Rust embedding. The final local measurements were **1.974209 ms** for a control
request/response while real TCP bulk reads were stalled, and **2.133 ms** for
bounded-duplex bulk backpressure. The proposed scratch control target is <=1
second. These measure separate session control service only, with one compact
inventory response; they do not measure engine/fleet/fanout capacity.

Coverage includes cross-peer binding substitution; old control/reconnect and
incarnation fences; bad role/version/daemon/generation; unanchored and mismatched
canonical policies; expiry after authentication; revocation/generation update
on existing streams; idle read wakeups; concurrent admissions and subscription
release; cancellation of idle control reads, handshake validation, accepted
blocking effects and admin changes; and held revocation authority through a
blocked bounded bulk write. A synthetic SQLite running record confirms revoke
clears controller but preserves Running state/capacity, without launching a real
process. The gated handshake-validation cancellation test proves the role permit
stays in the actual closure: thirty-two further attempts reject for capacity
while bulk permits remain untouched. It uses a test-only gate around real policy/
SQLite validation, not a mock store or ledger.

Red evidence: the initial budget/API test failed with the empty module; the idle
control cancellation regression failed before its binding fence; the output
producer test failed before its guarded API existed; and handshake cancellation
failed against the old waiter-owned admission lease, then passed after moving
that lease into the validation owner. The inspect-only InputReceipt mapping test failed while classified as Input, then passed after switching to Inspect; removing Inspect still denies it.

## Explicit remaining acceptance

Parent integration must perform exact current export generation, receiver resource,
agent/run ownership and source lookup inside these guarded callbacks. Full per-agent
control epochs and actual PTY dispatch serialization remain parent obligations;
this module does not prove those integration paths. Response correlation/schema
and inventory filtering must be implemented by that dispatcher using the checked
policy. Every authorized observation needs its own current resource check.

Full 8-worker x 30-agent host fanout/engine load acceptance remains unproved.
Native macOS strict process-tree lifetime remains blocked by platform decision
and proof. There is no OS process-lifetime or agent-sandbox claim, no listening
by default, no remote execution enablement, no installed binary replacement,
and no signals sent to real user daemons/agents.

## Discovery and bounded status delta

`Hello::Discover { version, target_daemon }` and `GenerationDiscovery { version,
daemon_id, generation }` have strict <=2048-byte decoders with no unknown fields.
Discovery uses the same independent mTLS `Role::Control` ALPN endpoint as ordinary
Control. `accept_incoming(transport, io) -> Accepted<S>` reads exactly one TLS/Hello
and returns either `Session(Session<S>)` or `Discovery` after a bounded response
and stream drop. `accept` remains strict, including Bulk; `accept_discovery` is an
optional discovery-only convenience, with no separate port or extra negotiation.
Discovery is transient and consumes the existing bounded control connection
permit through actual blocking policy/ledger validation and response write. It
mints no control binding, incarnation, subscription, writer or operation window,
and does not replace an existing binding.

`ControlClient::discover(transport, io, exact_receiver_pin, daemon, limits)`
validates the exact TLS server pin, receiver target/version and bounded response.
The returned generation is a reconnect hint, not authorization. Use NEW streams
for a fresh ordinary strict Control+Bulk pair. If generation changes again that
handshake rejects; there is no automatic mutation/input replay.

The narrow store discovery accessor obtains generation from the actual TLS peer's
ledger record and invokes the same canonical policy/anchor/expiry check under the
retained shared barrier. Revoked, corrupt, missing, unanchored and expired policy
denies. SQLite reopen advances generation and fences old bindings/windows/writers;
read-only reconciliation still requires current grants, exact resource ownership
and stable peer identity checks. Actual private SQLite reopen tests retain unknown
input receipts and lifecycle receipts while rejecting stale control, old bulk
bindings, closed-window new work and old writer/dispatch use. Records are synthetic
and no actual process/PTY is started, so native lifetime acceptance stays blocked.

Status now carries `after_agent: Option<String>` (serde default None) and
`limit: u16` (serde default 64), validated 1..=64. The cursor is a bounded identifier
and cannot accompany `agent_id: Some(...)`. Exact-agent status remains supported.
No unbounded ledger/status scan is exposed over this wire surface.

Delta checks: core wire tests (9) and session tests (22), real TLS plus private
SQLite, with strict all-target engine Clippy. New red tests failed before the
Discover/GenerationDiscovery/pagination/demultiplex APIs were implemented. The
new cancellation gate confirms a cancelled discovery waiter cannot release its
actual validation owner's control permit or create a binding. Malformed version,
wrong pin/target/role, inactive/damaged grants and substituted/oversized discovery
responses reject. Parent metadata/read dispatch and listener lifecycle remain
separate producer/integration work.
