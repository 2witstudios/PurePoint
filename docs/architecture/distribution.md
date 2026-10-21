# Distribution

**Maturity: SPECIFIED**

## Context

PurePoint needs to ship to users as a self-contained product with no external runtime dependencies. The distribution model covers how the app, daemon, and CLI are packaged, installed, and updated.

## Decisions

! [DIST-001] Daemon embedded in app bundle. The `pu-engine` binary is compiled via a Run Script build phase in Xcode and placed at `Contents/MacOS/pu-engine` alongside the main app executable. Debug builds compile for the host architecture only; release builds create a universal binary (ARM64 + x86_64) via `lipo`. Code signed with the app's identity. DaemonLifecycle checks the app bundle first, then PATH, then ~/.cargo/bin (development fallback). No external runtime dependencies — the app is a single drag-and-drop download.

! [DIST-004] Linux servers get static musl builds of `pu` and `pu-engine` (`x86_64-` and `aarch64-unknown-linux-musl`), one `purepoint-<version>-<target>.tar.gz` (plus `.sha256`) per target, attached to the same GitHub release as the DMG. They are built natively per architecture in `release.yml`. `scripts/install.sh` verifies the checksum and installs both binaries side by side in `~/.pu/bin`, replacing each by rename so a running daemon's binary is never overwritten in place. There's no package-manager integration yet.

! [DIST-005] Releases are built and signed in separate jobs (`.github/workflows/release.yml`, run only by a `v*` tag push). `build-macos` and `linux` compile every third-party dependency with a read-only token, no secrets and no restored cache; the macOS app is archived ad-hoc signed. `sign-macos` runs in the protected `release` environment and is the only job that receives secrets. It runs no build code: only Apple's tools, `.github/scripts/sign-macos-app.sh` (inside-out Developer ID signing, then a check that every Mach-O in the bundle has the identity, the hardened runtime and a secure timestamp), and the Sparkle and create-dmg releases, each fetched with `curl -fsSL` and checked against a SHA-256 committed in the workflow. The Sparkle tools match the version the app links in `Package.resolved`. The certificate is imported into a temporary keychain as non-extractable and usable only by `/usr/bin/codesign`, the keychain locks after 15 idle minutes, and the keychain and `.p12` are deleted after the last signature and again in an `always()` cleanup step. `publish` is the only job with `contents: write`. It checks out no code, creates the GitHub release with the DMG and Linux tarballs in one step, and never replaces a versioned asset. The `appcast.xml` on the `appcast` release is the moving update feed and is the one asset replaced on each release. The unsigned app and the DMG pass between jobs as workflow artifacts kept for 14 days, so a release can wait that long for environment approval and still be re-run. If the release already exists when `publish` runs again, every asset must match the published one byte for byte (by SHA-256), or the job fails before touching the appcast. Re-running every job re-signs the DMG and changes its bytes, so use "Re-run failed jobs", or delete the release first. Every action in every workflow is pinned to a full commit SHA with a version comment, and no checkout keeps the token in `.git/config`. `.github/scripts/test_release_workflow.py` checks these properties in CI.

### Repository settings the owner maintains

These are GitHub settings, not files, so the workflow can't enforce them. They're needed for DIST-005 to hold:

1. **`release` environment, before the next tag.** Create it (Settings → Environments), add the owner as a required reviewer, and limit deployment refs to tags matching `v*`. Move `APPLE_CERTIFICATE_P12`, `APPLE_CERTIFICATE_PASSWORD`, `KEYCHAIN_PASSWORD`, `APPLE_ID`, `APPLE_APP_PASSWORD`, `APPLE_TEAM_ID` and `SPARKLE_ED25519_KEY` from repository secrets to environment secrets, then delete the repository-level copies. If the environment doesn't exist when a tag is pushed, GitHub creates it with no protection, and `sign-macos` still reads the repository-level secrets. The release then works as before, but without the approval gate.
2. **Tag ruleset for `v*`.** Settings → Rules → Rulesets → New tag ruleset targeting `v*`: restrict creations, updates and deletions to the owner (bypass list), and block force pushes. Only the owner can then start a release.
3. **Immutable releases.** Settings → General → Releases → enable immutable releases, so a published release's assets and tag can't be changed. Enable it after the `appcast` release exists. It only applies to releases published afterwards, so the `appcast` release stays updatable.
4. **Actions policy.** Settings → Actions → General: turn on "Require actions to be pinned to a full-length commit SHA", and allow only GitHub-owned actions plus the ones the workflows use (`dtolnay/rust-toolchain`, `Swatinem/rust-cache`, `EmbarkStudios/cargo-deny-action`, `taiki-e/install-action`). Keep the default `GITHUB_TOKEN` permission read-only.

## Open Questions

! [DIST-002] Preserve canonical Pi private auth/session/settings data outside the bundle and use versioned private Application Support managed/trust metadata — avoids duplicate credentials and survives updates. Legacy remote shared-token clients deliberately enroll again; never downgrade remote authorization.

! [DIST-003] Stop and await app-owned Point Guard children before executable replacement, then restore native session/provider/trust state — app lifetime satisfies owner scope without rewriting worker-daemon supervision. Never adopt or stop unknown processes. Protected production signing/notarization remains owner-controlled.

## Design Directions

- macOS as primary platform
- No external runtime dependencies in final product
- Support for multiple CPU architectures
- Signed and verified for distribution

## Research Notes

DIST-003 partially answered: updating the app updates the daemon because it's embedded. Running daemon during update: the app sends Shutdown before quit. If the daemon was started by CLI in standalone mode, the update only affects the bundled copy — the standalone binary in PATH is managed separately (e.g. cargo install).

### [DIST-002] Point Guard state migration
**Researched: 2026-10-09**

Baseline bridge/setup.js discovers developer-home skills and main.js requires an explicit repository cwd. Its Pi 1.1.0 adapter reads native sessions and credentials. App-only preferences would diverge from CLI Pi; copying native auth/session data into a bundle would lose updates. Preserve canonical private ~/.pi/agent auth/settings/models/sessions and place managed runtime/identity metadata outside the bundle in private Application Support. Legacy remote shared-token pairing cannot safely identify individual devices; require deliberate QR v1 enrollment while preserving transcript/draft recovery. Bundle versioned Point Guard instructions and complete pu skills/reference support with explicit bundle-relative CLI/runtime paths.

Candidate: preserve native Pi storage and version managed/trust state, failing closed on malformed/unknown schema. Alternative explicit all-state import adds copies/secret migration risk; retained only as future user-requested migration.

### [DIST-003] App-lifetime bridge and updates
**Researched: 2026-10-09**

Existing bridge owns one Node Pi RPC process. App-lifetime Process ownership meets the approved lifetime boundary; launchd would add persistent supervision outside scope. Keep state and TLS identity outside the bundle, stop and await only owned children before replacement, and never adopt/kill an unknown listener. Candidate: app-owned process with exclusive private startup lock and bounded recovery. Alternative launchd deferred.

Official Node distribution supplies Darwin arm64/x64 binaries and SHA256 manifests: [Node releases](https://nodejs.org/download/release/). Pi requires Node >=22.19 and supports script-free npm installation: [Pi upstream](https://github.com/earendil-works/pi). Pin runtime and complete transitive production dependencies; relocate packaged artifact outside checkout and prove empty-HOME/stripped-PATH launch for both advertised architectures.

Apple requires nested code to be signed and hardened-runtime entitlements to follow the executable: [TN2206](https://developer.apple.com/library/archive/technotes/tn2206/_index.html), [JIT on Apple Silicon](https://developer.apple.com/documentation/Apple-Silicon/porting-just-in-time-compilers-to-apple-silicon). Inside-out signing and Node allow-jit must be checked on the artifact. Ad-hoc CI signature/execution proof does not establish [Developer ID/notarization](https://developer.apple.com/documentation/security/notarizing-macos-software-before-distribution); that remains protected owner evidence.

## Requirements and Interfaces

- Given a fresh supported Mac, should provide bundle-relative Node/Pi/bridge/pu and complete instructions/resources without external development tools.
- Given a packaged candidate, should prove both advertised architectures under empty HOME and stripped PATH after relocation outside checkout.
- Given an update, should stop owned managed children before replacement and preserve native auth/session/managed trust state.
- Given ad-hoc CI success, should retain protected Developer ID/notarization and owner fresh-device acceptance as separate pending proof.
- Bundle layout, runtime manifest, lifecycle/state/auth contracts and collision/recovery edge cases: `../product/point-guard-setup.md` PRPG r1 / PRPG-TRUST-1.

## Edge Cases

Missing architecture/resource/signature blocks package acceptance; no developer-path fallback in distribution. Unknown listeners are never adopted/killed. Unknown/corrupt state is never silently migrated to weaker authorization.
