# Distribution

**Maturity: CONVERGING**

## Context

PurePoint needs to ship to users as a self-contained product with no external runtime dependencies. The distribution model covers how the app, daemon, and CLI are packaged, installed, and updated.

## Decisions

! [DIST-001] Daemon embedded in app bundle. The `pu-engine` binary is compiled via a Run Script build phase in Xcode and placed at `Contents/MacOS/pu-engine` alongside the main app executable. Debug builds compile for the host architecture only; release builds create a universal binary (ARM64 + x86_64) via `lipo`. Code signed with the app's identity. DaemonLifecycle checks the app bundle first, then PATH, then ~/.cargo/bin (development fallback). No external runtime dependencies — the app is a single drag-and-drop download.

## Open Questions

? [DIST-002] What is the migration path for existing users?
Existing users may have legacy project directories and data. Should migration be automatic, an explicit command, or handled by the app?

? [DIST-003] How should auto-update work?
If the daemon is embedded in the app, updating the app updates the daemon too. But what if the daemon is running when the update happens? How are running processes handled during updates?

## Design Directions

- macOS as primary platform
- No external runtime dependencies in final product
- Support for multiple CPU architectures
- Signed and verified for distribution

## Research Notes

DIST-003 partially answered: updating the app updates the daemon because it's embedded. Running daemon during update: the app sends Shutdown before quit. If the daemon was started by CLI in standalone mode, the update only affects the bundled copy — the standalone binary in PATH is managed separately (e.g. cargo install).

### [DIST-002] Point Guard state migration
**Researched: 2026-10-09**

Baseline bridge/setup.js discovers developer-home skills and main.js requires an explicit repository cwd. Its Pi 1.1.0 adapter reads native sessions and credentials. App-only preferences would diverge from CLI Pi; copying native auth/session data into a bundle would lose updates. Preserve canonical private ~/.pi/agent auth/settings/models/sessions and place managed runtime/identity metadata outside the bundle in private Application Support. Legacy remote shared-token pairing cannot safely identify individual devices; require deliberate QR v2 enrollment while preserving transcript/draft recovery. Bundle versioned Point Guard instructions and complete pu skills/reference support with explicit bundle-relative CLI/runtime paths.

Candidate: preserve native Pi storage and version managed/trust state, failing closed on malformed/unknown schema. Alternative explicit all-state import adds copies/secret migration risk; retained only as future user-requested migration.

### [DIST-003] App-lifetime bridge and updates
**Researched: 2026-10-09**

Existing bridge owns one Node Pi RPC process. App-lifetime Process ownership meets the approved lifetime boundary; launchd would add persistent supervision outside scope. Keep state and TLS identity outside the bundle, stop and await only owned children before replacement, and never adopt/kill an unknown listener. Candidate: app-owned process with exclusive private startup lock and bounded recovery. Alternative launchd deferred.

Official Node distribution supplies Darwin arm64/x64 binaries and SHA256 manifests: [Node releases](https://nodejs.org/download/release/). Pi requires Node >=22.19 and supports script-free npm installation: [Pi upstream](https://github.com/earendil-works/pi). Pin runtime and complete transitive production dependencies; relocate packaged artifact outside checkout and prove empty-HOME/stripped-PATH launch for both advertised architectures.

Apple requires nested code to be signed and hardened-runtime entitlements to follow the executable: [TN2206](https://developer.apple.com/library/archive/technotes/tn2206/_index.html), [JIT on Apple Silicon](https://developer.apple.com/documentation/Apple-Silicon/porting-just-in-time-compilers-to-apple-silicon). Inside-out signing and Node allow-jit must be checked on the artifact. Ad-hoc CI signature/execution proof does not establish [Developer ID/notarization](https://developer.apple.com/documentation/security/notarizing-macos-software-before-distribution); that remains protected owner evidence.
