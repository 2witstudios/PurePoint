# Maintained TLS transport foundation

This opt-in library does not create listeners, start services, persist trust or dispatch
operations. It establishes certificate possession only. Remote execution and native
macOS process-lifetime acceptance are not enabled or proved here.

## Public integration surface

- `Identity::generate() -> Result<Identity, Error>` explicitly creates an identity.
- `Identity::restore(cert_der: &[u8], key_pkcs8_der: &[u8])` validates the restored
  certificate/public key against its private key using rustls. Restoration never
  regenerates. Expired/not-yet-valid certificates are rejected by TLS verification.
- `certificate_der()`, `private_key_der()` explicitly export persistence bytes;
  `peer_id()` returns the SHA-256 fingerprint of the whole certificate DER.
- `PeerId::from_certificate_der(cert_der)` derives an expected server pin from
  public enrollment bytes without requiring the remote private key. This is not
  possession evidence; only a successful connection supplies that evidence.
- `EnrolledPeers::new(iter_of_cert_der)` requires explicit exact certificate pins.
  Empty/malformed enrollment fails closed. No OS/web roots are loaded.
- `MeshTransport::new(&identity, enrollment)` builds private, non-mutable TLS configs.
- `accept(io, Role::{Control,Bulk})` / `connect(io, role, expected_server: PeerId)`
  take ownership of an existing async stream and return `AuthenticatedConnection<S>`.
  Both verify the exact enrolled leaf before exposing any application frame.
- Connections expose `peer_id()`, `role()`, `protocol()` and
  `read_frame(deadline)` / `write_frame(payload, deadline)` only. No raw stream or
  caller-supplied authenticated principal is exposed.

The wire format is a four-byte big-endian unsigned payload length followed by
opaque bytes. Zero and oversized lengths fail before body allocation. Limits are
64 KiB control and 256 KiB bulk, fixed by authenticated role. The deadline covers
header, body and (for writes) flush. Timeout, cancellation, truncation, bad length
or I/O error permanently poison **both** directions. Drop the connection to close;
never retry or resume its partially consumed frame. Successful flush is not an
operation receipt. Payload/schema validation remains with the bridge.

## Security and parent obligations

TLS 1.3 only; handshake deadline 5 seconds. Standard rustls WebPki validators
check validity and certificate possession against explicitly configured roots;
exact byte pin checks then reject certificates that merely chain successfully,
including a reissued certificate with the same key. There is no custom verifier,
custom cryptography or plaintext fallback. `mesh.invalid` is a fixed generated SAN
for standard TLS name validation, never a principal or an address. Addresses may
change without changing certificate identity.

Early data, client resumption, server session caching and TLS 1.3 tickets are
disabled. The default rustls server ticketer is disabled and tested. Each
connection must perform fresh certificate authentication. Distinct TLS-protected
ALPN values `pu-mesh-control/1` and `pu-mesh-bulk/1` prevent role confusion;
missing/mismatched ALPN fails closed. These suffixes identify transport roles,
not the parent's bridge wire version.

The parent must persist secrets in its approved private-directory boundary,
perform local-admin enrollment/rotation, bound total sessions/queues, bind control
and bulk sessions to peer generation, receiver incarnation and opaque control
binding, and validate bridge versions/payloads. TLS identity is immutable transport
evidence, **not** a live grant. Revalidate live authority for every request/event
and hold the revocation barrier through actual dispatch/send. Old transport configs
must not restore revoked authority; close sessions and replace enrollment configs
under the parent's revocation policy. No forward delegation is provided.

## Library choices and official references

Resolved versions: rustls **0.23.45**, tokio-rustls **0.26.6**, rcgen **0.14.10**,
ring **0.17.14**, sha2 **0.10.9**. rustls/tokio-rustls use the maintained ring
provider explicitly; AWS-LC and platform roots are not enabled. TLS 1.2 support is
enabled only as a dev dependency to test downgrade rejection; all transport configs
allow TLS 1.3 exclusively. These versions' manifests and the resolved host engine
dependency closure fit MSRV 1.89 and the repository's allowed licenses.

- [rustls ServerConfig](https://docs.rs/rustls/0.23.45/rustls/struct.ServerConfig.html)
  documents session storage, ticket and early-data settings.
- [rustls ClientConfig](https://docs.rs/rustls/0.23.45/rustls/struct.ClientConfig.html)
  documents resumption and early-data controls.
- [rustls CertifiedKey](https://docs.rs/rustls/0.23.45/rustls/sign/struct.CertifiedKey.html)
  provides `from_der` and explicit `keys_match` validation.
- [rustls WebPkiClientVerifier](https://docs.rs/rustls/0.23.45/rustls/server/struct.WebPkiClientVerifier.html)
  supplies required client certificate verification without a permissive verifier.
- [tokio-rustls](https://docs.rs/tokio-rustls/0.26.6/tokio_rustls/)
  provides asynchronous stream handshakes.
- [rcgen](https://docs.rs/rcgen/0.14.10/rcgen/) supplies self-signed identity generation.

Official downloaded crate source was inspected for APIs and security defaults,
including rustls `crypto/signer.rs`, `server/server_conn.rs` and `common_state.rs`.
No private key is logged or derives Debug/serialization.

## Verification

Tests are colocated in `tests.rs` and use independent scratch TCP loopback endpoints.
They cover mutual identity, stable restore/address changes, corrupt/mismatched
material, unknown/missing/expired/future clients and servers, exact same-key
reissue pins, wrong server pin, untrusted self-reported identity, role/ALPN
separation, handshake deadlines, TLS 1.2 rejection on both ends, repeated full
handshakes/no early data or tickets, frame boundaries, malformed lengths,
truncation, and partial read/write timeout/cancellation poisoning.

TDD evidence: initial missing-Identity test failed compilation; it passed after
identity implementation. The loopback authentication test then failed for missing
transport API and passed after implementation. The frame roundtrip test failed for
missing framing methods and passed after implementation. All cargo commands used
`CARGO_TARGET_DIR=/tmp/purepoint-mesh-transport-target`; no installed binaries,
actual daemons, app builds or other worktrees were touched.

Final check results on this candidate:

- Focused `cargo test -p pu-engine --lib mesh_transport`: **23 passed**.
- Full `cargo test -p pu-engine --lib --locked`: **204 passed**.
- `cargo clippy -p pu-engine --all-targets -- -D warnings`: **passed**.
  Its first run caught four boolean-assert style warnings; those were fixed.
- `cargo +1.89.0 check -p pu-engine --lib --locked`: **passed**, production
  dependency features (without the TLS 1.2 test fixture feature).
- `cargo fmt --all -- --check` and `git diff --check`: **passed**.
- Resolved host engine dependency closure: **117 packages**, no declared MSRV
  above 1.89 and no license outside the repository's allow list. `cargo deny` is
  not installed, so this is a manifest/metadata check, not a cargo-deny advisory
  audit.
- The public-certificate pin constructor also had an observed compile-red test
  before implementation, followed by green focused and full checks.

No independent review is claimed; parent owns review, PR, task records and
integration into `pu/same-network-bridge`. Transport tests do not establish
process lifetime, composed revocation, connection-budget or execution acceptance.
