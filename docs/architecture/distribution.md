# Distribution

**Maturity: SPECIFIED**

## Context

PurePoint needs to ship to users as a self-contained product with no external runtime dependencies. The distribution model covers how the app, daemon, and CLI are packaged, installed, and updated.

## Decisions

! [DIST-001] Daemon embedded in app bundle. The `pu-engine` binary is compiled via a Run Script build phase in Xcode and placed at `Contents/MacOS/pu-engine` alongside the main app executable. Debug builds compile for the host architecture only; release builds create a universal binary (ARM64 + x86_64) via `lipo`. Code signed with the app's identity. DaemonLifecycle checks the app bundle first, then PATH, then ~/.cargo/bin (development fallback). No external runtime dependencies — the app is a single drag-and-drop download.

! [DIST-004] Linux servers get static musl builds of `pu` and `pu-engine` (`x86_64-` and `aarch64-unknown-linux-musl`), one `purepoint-<version>-<target>.tar.gz` (plus `.sha256`) per target, attached to the same GitHub release as the DMG. They are built natively per architecture in `release.yml`. Each tarball also gets a minisign signature (`.tar.gz.minisig`) from the release key and a GitHub build provenance attestation. Each tarball also carries the static `pu-point-guard-lock` helper that the phone access payload ships (DIST-008). `scripts/install.sh` checks the checksum, then verifies who built the download before installing anything: the signature must verify against the minisign public key embedded in `install.sh` (with `minisign`, or with OpenSSL 3 when minisign isn't installed), and when the GitHub CLI is installed and signed in, `gh attestation verify --repo 2witstudios/PurePoint` must pass. At least one of the two must succeed and any check that runs must pass, so neither check depends only on files from the same release. If neither can run (no signature on the release, or no minisign, OpenSSL 3 or `gh`), the install stops unless `PU_INSTALL_SKIP_VERIFY=1` is set, which installs with a warning. The opt-out never overrides a check that failed. A local `PU_ARCHIVE` is installed as given. The script installs both binaries side by side in `~/.pu/bin`, replacing each by rename so a running daemon's binary is never overwritten in place. There's no package-manager integration yet.

! [DIST-005] Releases are built and signed in separate jobs (`.github/workflows/release.yml`, run only by a `v*` tag push). `build-macos` and `linux` compile every third-party dependency with a read-only token, no secrets and no restored cache; the macOS app is archived ad-hoc signed. `sign-macos` runs in the protected `release` environment and is the only job that receives secrets. It runs no build code: only Apple's tools, `.github/scripts/sign-macos-app.sh` (inside-out Developer ID signing, then a check that every Mach-O in the bundle has the identity, the hardened runtime and a secure timestamp), and the Sparkle and create-dmg releases, each fetched with `curl -fsSL` and checked against a SHA-256 committed in the workflow. The Sparkle tools match the version the app links in `Package.resolved`. The certificate is imported into a temporary keychain as non-extractable and usable only by `/usr/bin/codesign`, the keychain locks after 15 idle minutes, and the keychain and `.p12` are deleted after the last signature and again in an `always()` cleanup step. `build-macos` uses the same Node (22.23.3, via `actions/setup-node`) as CI, and every `xcodebuild` that builds the app in CI or a release passes `-onlyUsePackageVersionsFromResolvedFile`, so Swift packages resolve only to the committed `Package.resolved`. Every remote Swift package is pinned to a version or a revision, never a branch. `create-dmg` failures fail the release: a non-zero exit is tolerated only when the DMG was still written and passes `hdiutil verify`, with a warning. `sign-linux` also runs in the `release` environment. It signs the Linux tarballs with `.github/scripts/minisign.sh` (minisign-format Ed25519 signatures made with OpenSSL, so no signing tool is downloaded) after checking that the `RELEASE_MINISIGN_KEY` secret matches the public key in `scripts/install.sh`. Without that secret the job skips signing with a warning and the release ships without `.minisig` files. `sbom` writes CycloneDX SBOMs for `pu` and `pu-engine` (`cargo cyclonedx --target all`, pinned version) and for the Point Guard npm runtime (`npm sbom --omit=dev`). `attest` (`id-token`, `attestations` and `artifact-metadata` write, no checkout, no `run` steps) records build provenance for the Linux tarballs and the DMG with `actions/attest`, and attests the SBOMs against them; verify with `gh attestation verify <file> --repo 2witstudios/PurePoint`. `publish` is the only job with `contents: write` and runs only after both signing jobs and `attest` succeed. It checks out no code, creates the GitHub release with the DMG, the Linux tarballs and phone access payloads (DIST-008) and their signatures, and the SBOMs in one step, and never replaces a versioned asset. The `appcast.xml` on the `appcast` release is the moving update feed and is the one asset replaced on each release. The unsigned app and the DMG pass between jobs as workflow artifacts kept for 14 days, so a release can wait that long for environment approval and still be re-run. If the release already exists when `publish` runs again, every asset must match the published one byte for byte (by SHA-256), or the job fails before touching the appcast. Re-running every job re-signs the DMG and changes its bytes, so use "Re-run failed jobs", or delete the release first. Every action in every workflow is pinned to a full commit SHA with a version comment, and no checkout keeps the token in `.git/config`. `.github/scripts/test_release_workflow.py` checks these properties in CI, and `.github/scripts/test_install_script.py` checks the install script's verification against signatures made by `minisign.sh` (and, when `minisign` is installed, that minisign itself accepts them).

! [DIST-006] The update feed is signed. `sign-macos` generates `appcast.xml` and signs it with `sign_update` and the Sparkle EdDSA key that signs the DMG, so the feed and the update share one key. The app sets `SURequireSignedFeed` and `SUVerifyUpdateBeforeExtraction` (both need Sparkle 2.9 or later), so Sparkle rejects an unsigned or modified feed and checks the update's EdDSA signature before extracting it. With `SUVerifyUpdateBeforeExtraction`, the EdDSA key can't be rotated in an ordinary update; rotating it needs a release that turns the setting off first. Apps released before this change don't check the feed signature but still check each update's EdDSA signature and Apple team.

! [DIST-007] Before it starts the Point Guard bridge, a Release build of the app checks its own bundle with `SecStaticCodeCheckValidity` (strict validation, all architectures, nested code) against the designated requirement of the running app. This covers the resource seal over the bridge's JavaScript, `node_modules` and helpers, so a bundle modified after installation, even one re-signed by someone else, doesn't start the bridge: Point Guard shows "PurePoint's files were modified after it was installed" and asks for a reinstall. The check runs off the main thread. Debug builds skip it and log that they did. `apps/purepoint-macos/verification/check-bundle-seal.py` exercises it against throwaway ad-hoc signed bundles in CI.

! [DIST-008] Phone access for Linux hosts (`product/linux-phone-host.md` LPH-001/002) is a separate, opt-in release asset: `purepoint-pointguard-<version>-<x86_64|aarch64>-linux-gnu.tar.gz` plus `.sha256`, a `.minisig` signature and build provenance, one per arch, built by `package-runtime.py --layout dir`. Its single top-level directory holds:
- the official nodejs.org glibc Node, pinned by SHA-256 in `node-release.json`, as `bin/node`
- the musl `pu-point-guard-lock` from the same release, as `bin/point-guard-lock`
- `bridge/`, production `node_modules` (only the target's optional natives, restored from lockfile integrity), `docs/` and `support/`
- `runtime-manifest.json` with `layout: "linux"`, the release version compiled into `pu` (`PU_RELEASE_VERSION`) as `puVersion`, and payload-relative paths only

The tarball has no links, nothing group/other-writable, root-owned entries and a fixed mtime. In `release.yml`, `linux-phone` builds each payload from the `linux` job's verified tarball and proves it: relocated, and installed by `install.sh` into a throwaway HOME, under an empty environment with a pinned-TLS enrollment. `sign-linux` signs the payloads with the release minisign key, and `attest` records their build provenance and attests the Point Guard npm SBOM against them, exactly as for the `pu` tarballs (DIST-004/005); `publish` waits for `linux-phone`.

`install.sh` installs the payload only with `PU_WITH_PHONE=1`, or `PU_PHONE_ARCHIVE=<local tarball>`, after `pu` and `pu-engine`:
- It refuses musl or glibc < 2.28 with "glibc ≥ 2.28 required for phone access", exiting 3 while `pu`/`pu-engine` stay installed.
- It verifies the SHA-256, then applies the DIST-004 rule to the payload: the minisign signature or `gh attestation verify` must succeed, any check that runs must pass, and only `PU_INSTALL_SKIP_VERIFY=1` installs an unverifiable download (with a warning). A local `PU_PHONE_ARCHIVE` is installed as given (its `.sha256` is checked when present).
- It rejects absolute, `..`, out-of-directory or oddly named entries and every link or special file.
- It unpacks into a fresh `~/.pu/pointguard/.install.*`, removes group/other write, and checks the manifest, the files `pu phone` runs, and that `bin/node` runs on the host.
- It renames the payload (`mv -T`) onto `~/.pu/pointguard/<puVersion>/`. An existing complete version is never modified, so a running bridge's files stay intact; an incomplete one is refused.

The next step it prints is `pu phone install --now`. `pu phone` runs only the payload whose `puVersion` equals its own.

### Repository settings the owner maintains

These are GitHub settings, not files, so the workflow can't enforce them. They're needed for DIST-005 to hold:

1. **`release` environment, before the next tag.** Create it (Settings → Environments), add the owner as a required reviewer, and limit deployment refs to tags matching `v*`. Move `APPLE_CERTIFICATE_P12`, `APPLE_CERTIFICATE_PASSWORD`, `KEYCHAIN_PASSWORD`, `APPLE_ID`, `APPLE_APP_PASSWORD`, `APPLE_TEAM_ID` and `SPARKLE_ED25519_KEY` from repository secrets to environment secrets, then delete the repository-level copies. If the environment doesn't exist when a tag is pushed, GitHub creates it with no protection, and `sign-macos` still reads the repository-level secrets. The release then works as before, but without the approval gate.
2. **Tag ruleset for `v*`.** Settings → Rules → Rulesets → New tag ruleset targeting `v*`: restrict creations, updates and deletions to the owner (bypass list), and block force pushes. Only the owner can then start a release.
3. **Immutable releases.** Settings → General → Releases → enable immutable releases, so a published release's assets and tag can't be changed. Enable it after the `appcast` release exists. It only applies to releases published afterwards, so the `appcast` release stays updatable.
4. **Linux signing key.** `scripts/install.sh` embeds the minisign public key `RWSEvitv5QIgchUQgebD93PuXamKznz5sMJMavmViuMEYSM8nVtcrRv6`. Store the matching private key (an Ed25519 PEM file, created with `.github/scripts/minisign.sh keygen <file>`) as the `RELEASE_MINISIGN_KEY` secret of the `release` environment (`gh secret set RELEASE_MINISIGN_KEY --env release < <file>`), keep an offline backup, and delete the local copy. Until the secret exists, releases ship without Linux signatures and the release job warns about it; `install.sh` then needs a signed-in `gh` or `PU_INSTALL_SKIP_VERIFY=1`. To replace the key, generate a new one, update `RELEASE_PUBLIC_KEY` in `scripts/install.sh` and the secret together; the signing job refuses a secret that doesn't match the embedded key.
5. **Actions policy.** Settings → Actions → General: turn on "Require actions to be pinned to a full-length commit SHA", and allow only GitHub-owned actions plus the ones the workflows use (`dtolnay/rust-toolchain`, `Swatinem/rust-cache`, `EmbarkStudios/cargo-deny-action`, `taiki-e/install-action`). Keep the default `GITHUB_TOKEN` permission read-only.

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
