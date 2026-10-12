# Owner-local administration service

`daemon_lifecycle::mesh_admin::MeshAdminService` is an opt-in library, declared
as a lifecycle child so it uses the existing validated private directory and
retained `DaemonLock` without adding a second filesystem authority. Its
constructor performs no initialization or filesystem writes. Root supplies
`Limits`, `SessionLimits`, export limits and locally approved runtime IDs.

`handle(work, LocalAdmin, MeshAdminRequest)` owns accepted work through caller
cancellation. Local ingress must derive `LocalAdmin` from authenticated owner
UID and local policy. The DTO has no asserted UID/admin flag; bridge wire
requests have no administration variant. CLI `pu mesh` is a thin local IPC
client. Protocol remains 8: an older daemon that does not recognize the additive
request fails explicitly; the CLI does not restart or downgrade it.

Only `identity initialize` creates identity, configuration and ledger state.
Retained mesh metadata prevents initialization even if the identity is missing.
Existing identity, ledger or configuration corruption/missing state is never
repaired automatically. Initialization failure can leave retained partial state;
explicit owner recovery is required. Public responses contain only daemon ID,
certificate DER, fingerprint, policies, opaque export descriptions and listener
configuration, never private keys. Policies and grants in local responses are
metadata, never authority for an effect.

Manual `enroll-manual` requires exact public DER plus a separately supplied
lowercase SHA256 certificate fingerprint. Verify that fingerprint independently
out of band before enrollment. There is no invitation exchange or acceptance
implementation in this slice. The manual OOB gap is deliberate and labelled in
CLI help. Grants name locally issued, active exports and receiver-approved
runtime IDs; no peer executable or shell configuration is accepted. Grant
validation uses the authority's guarded export read. Export revocation can race
between that check and policy publication, leaving a grant to a revoked immutable
ID; current dispatch/event export checks must deny it. IDs are never recycled.

Policy update preserves revocation. Regrant and generation advance are explicit,
expected-generation changes inside exclusive authority. Revoke also checks the
exact current generation there, but does not require a valid policy file: a
strict fingerprint selector targets the authoritative ledger record and session
notification without claiming TLS authentication. A damaged policy can be
explicitly repaired with certificate + pin + generation; repair still preserves
revocation. Running worker execution is not stopped or released by these changes.

All post-init mutations use `SessionAuthority::administer` or
`administer_exports`/export wrappers. Configuration publication runs inside the
exclusive export hook, which fences all bindings on success/error/unwind before
notification. Closing existing bindings is not a listener-admission fence: an old
listener can still begin a new handshake until the root coordinator stops/drains
it. Root must serialize local configuration changes with that stop/drain and
must not report applied disablement from the service response alone. No listener
hook or applied-state DTO is supplied by this module. A private random temporary file is created exclusively, fsynced,
checked, atomically renamed and followed by directory fsync. Destination and
private-directory identity checks reject unsafe/replaced entries. This relies
on the trusted same-account model; it is not a sandbox against malicious code
with that identity. A failed publication preserves recovery diagnostics, can
leave durable intent whose acknowledgement is uncertain, and triggers refresh.

`Config` responses describe **persisted intent**, not applied/bound listeners.
Disabled means both addresses are absent. Enabled intent requires two specific
literal addresses with the same IP and distinct nonzero ports. Wildcard,
multicast/broadcast addresses and incomplete pairs reject. Loopback is permitted.
No default port arithmetic or hostname/interface fallback occurs.

`listener_snapshot(work,admin)` returns `None` for disabled intent, otherwise
`ListenerSnapshot { authority: Arc<SessionAuthority>, transport: MeshTransport,
config, revision }`. Transport comes from the authority's guarded, bounded,
canonical certificate set; an enabled configuration with no active enrolled
pins cannot construct a listener snapshot. This snapshot is handshake setup,
not authorization. The root manager must stop the old service, bind **both**
addresses before publishing applied state, and report applied bind failures.
No listener, transport task, worktree or process is created here.

`current_authority(work,admin)` returns the same live authority for trusted local
embedding, including root read routing. Callers use its guarded callbacks for
every observation/effect. `subscribe_revision()` reports refresh events after
changes and failures. Refresh even when the numeric revision is unchanged:
failed publication must cause a new setup check and root must close affected
service on failure. Successful mutations increment durable config revision;
peer/export changes also refresh transport setup. Authority is checked afresh
inside sessions and events regardless of the transport's known certificate pins.

Local peer pages use `Ledger::peers_page` exclusively, checking each canonical
policy/anchor even when revoked/expired. Request count is 1..256. Retained peer
DTOs have a 512 KiB serialized budget, with `next_after` at the last included ID;
no 256-large-policy allocation precedes that bound. Certificates are excluded
from policy list responses. CLI DER input is <=16 KiB and grants input <=64 KiB;
regular, non-symlink files only. Export catalogs retain at most the constructor
limit (ceiling 1024). Transport enumeration has the separate 256-peer limit and
must fail closed rather than silently truncate.

Scratch checks use the allocated `/tmp/purepoint-mesh-source-target`. Tests
exercise private filesystem and real SQLite reopen, actual authority hooks,
IPC serialization/parser behavior, and a scratch protocol-8 Unix responder.
No Xcode/Swift build, installed binary/config writes, invitations, real daemon
signals or native remote execution are part of this producer. Main/IPC/listener
integration and independent review belong to the parent. The full feature and
platform execution gate remain unchanged.
