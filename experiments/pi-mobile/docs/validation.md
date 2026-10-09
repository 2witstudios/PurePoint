# Validation evidence — 2026-10-08

- `npm test`: 25/25 passed. Covers core projection/framing, real child RPC, queue/reconnect/controller races and Stop dialog cancellation, native read-only history, authenticated network, full fixture flow, private QR page/payload behavior and isolated published vanilla Pi 1.1.0.
- `npm run check`: passed (`tsc --noEmit`, checkJs).
- `npm audit`: zero vulnerabilities after pinning ws 8.22.0.
- Local vanilla CLI `--version`: 1.1.0. Upstream checkout: ce950d78f424dcaf9f5d6a03ce80ab141130eb1d. Registry package and lockfile verified locally; no global install/link.
- Swift Foundation standalone logic executable: passed. Draft recovery, stale snapshot suppression, fenced-code splitting and endpoint policy.
- `swiftc -typecheck` on ChatDomain / PairingSecret / ChatModel: passed against the Mac SDK, emitting no application binary.
- `swiftc -frontend -parse` on iOS sources/test source: passed (syntax only).
- `plutil -lint` on standalone Xcode project and Info.plist: passed. Shared scheme and all file references checked. No shell build phases, PurePoint references or installed binary destinations.
- App mark: 1024×1024 RGB PNG, no alpha.
- QR addition: bridge checkJs passed, audit remains clean, Swift Foundation pairing parser checks passed (valid code, public endpoint, short credential, unsupported version, malformed data), and iOS scanner syntax/Info.plist checks passed. Scanner permission, camera recognition and scan-to-connect remain owner-device checks; no iOS SDK compilation was performed by the agent. User reported successful owner build/launch after the Section initializer fix, before this scanner addition.
- Owner OpenRouter authentication and a real model call: passed through the bridge's RPC child with native configuration, provider `openrouter` and model `~anthropic/claude-haiku-latest`. The prompt was accepted as started, returned “Pi Mobile real connection works.”, and settled without abort/error. This smoke test used `--no-session --no-tools`; it did not save a conversation or delegate workers. The alias's underlying model version was not independently verified.

TDD: initial framing/projection, RPC, controller and network suites were run red with absent implementations, then implemented and passed. Further regression tests cover race discoveries and the real native boundary. Tests are colocated in the standalone module; Swift XCTest domain tests ship for owner Cmd+U, with safe standalone execution of the same core behaviors here.

Not validated: Xcode/iOS SDK build or XCTest execution, SwiftUI rendering/screenshots, simulator/device installation, signing, physical-device keyboard/IME/accessibility, live phone-to-Mac tailnet connection, actual worker delegation. No xcodebuild, PurePoint Swift build, root Cargo build, daemon install/overwrite, deployment, merge, credential issuance or global executable changes were performed. Owner steps and acceptance exercise are in README.

## Scoped completion review

Structure and requirements: all files stay in this standalone module. Core projection has no network/process dependencies, native sessions remain authoritative, and clients never receive an arbitrary RPC/path operation. Fixtures are executable rather than TODO flows. The app, bridge, native launcher, requirements, lockfile, tests and owner instructions are present. UI/device acceptance remains explicitly unverified.

Test quality: isolated local fixtures, a real subprocess and a real published Pi boundary cover the core contract. Queue/reconnect/branch/Stop failure cases are behavioral tests. Performance is bounded by row/record/request/backlog budgets, coalesced snapshots and capped pending commands; slow sockets close and can resync.

Security review (OWASP 2021):

1. Access control: authenticated upgrade, single controller, semantic allowlist, session IDs resolved server-side.
2. Cryptography: Tailscale transport assumptions explicit; optional verified TLS; device Keychain stores the owner secret.
3. Injection: child executable/cwd/argument arrays; network never supplies a shell command, executable or raw session path.
4. Design: no automatic uncertain replay; run/epoch checks guard Stop and session mutations.
5. Configuration: reject wildcard/public binds and browser-origin upgrades; signing is owner-managed.
6. Components: exact vanilla/runtime and ws pins, lockfile, audit reports zero vulnerabilities.
7. Authentication: owner-provided private secret file, timing-safe comparison, no generated production credentials.
8. Integrity: native session files are not rewritten; read-only branch semantics and pinned runtime verified.
9. Logging: no authorization headers, secret contents, RPC transcript or raw stderr are logged; failures display setup guidance.
10. SSRF: app endpoints constrained to tailnet/loopback; no server-side arbitrary URL fetch operation.

Documentation covers interface, setup, constraints and recovery. No blocking finding remains in the safely verified bridge/domain scope; this is a self-review, not an independent review or evidence of iOS/device validation.
