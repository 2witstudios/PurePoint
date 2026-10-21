# Linux Phone Host Epic

**Status**: IMPLEMENTED on `pu/linux-phone-host` — awaiting the owner's Xcode build of the iOS and Mac apps, and the first `v*` release to exercise the release jobs
**Goal**: Pair an iPhone/iPad directly with a headless Linux PurePoint host using the same pinned-TLS, per-device trust model as the Mac.

## Overview

Users who run PurePoint on a cloud VM or home server can reach it from their phone today only through a Mac that stays on and forwards over SSH. This epic runs the existing Point Guard bridge headless on Linux under systemd, pairs the phone from a terminal QR, and manages devices with `pu phone`, without weakening the Mac's trust guarantees. Spec: `docs/product/linux-phone-host.md`. It is gated on that spec reaching DECIDED. Effort: S ≤ 1 day, M ≤ 3 days, L ≤ 1 week.

---

## Chat grant on the shared bridge

Delivered by PR #264 (PGSET-012): chat access derived from existing grants, enforced per message and per broadcast. This epic depends on it merging; no further work here. (Effort: none)

---

## Platform seam in the bridge runtime

Extract `platform.js` (state dir, host kind/label, admin transport, copy) and route `runtime.js` through it with no behavior change on darwin. (Effort: S)

**Requirements**:
- Given darwin, should produce byte-identical admin descriptor, paths and listener behavior to today's runtime tests.
- Given linux, should default the state dir to `${XDG_STATE_HOME:-~/.local/state}/purepoint/point-guard` and use host-neutral recovery copy.
- Given `POINT_GUARD_SUPERVISOR=systemd` and a busy lock from a draining predecessor, should retry within the bounded ownership window instead of failing immediately.


**Status**: Done. `bridge/platform.js` (state dir, host kind and label, admin transport, copy), `tailnet.js`, `remote.js`; `runtime.js` takes them as parameters. Darwin behavior is unchanged (full bridge suite passes on macOS); `POINT_GUARD_SUPERVISOR=systemd` gets the bounded `lock_busy` retry. Phone-facing copy is host-neutral.
---

## Unix-socket owner admin transport

Serve the existing admin dispatch over a Unix socket on Linux, still requiring the bearer token. (Effort: M)

**Requirements**:
- Given startup, should create the admin directory fresh at mode 0700, verify it with lstat, and set the socket's mode to 0600 after bind, whatever the umask.
- Given an existing admin path that is a symlink, foreign-owned or group/other-accessible, should refuse to start with actionable recovery.
- Given a request without the matching bearer token, an `Origin` header, or over the size/time limits, should reject it exactly as the loopback transport does.


**Status**: Done. `admin.js` `prepareAdminSocket`/`serveAdmin({socketPath})`; `admin.json` carries `adminSocket` only. `pu` checks directory, socket and token ownership and modes before sending the token. Proven in `prove-linux-runtime.mjs` and the systemd CI step.
---

## Tailnet discovery and late bind on Linux

Discover the tailnet IPv4 address from interfaces confirmed with `tailscale ip -4` and bind it, or use `tailscale serve` TCP passthrough in userspace mode, retrying in the background until the tailnet is available. (Effort: L)

**Requirements**:
- Given an interface address and `tailscale ip -4` that agree on one 100.64.0.0/10 address, should bind the remote listener to exactly that address.
- Given `tailscale ip -4` reporting an address no interface holds, should bind the device-trust listener on loopback, advertise `wss://<tailnet IP>:<port>/v1`, and pair only once the serve forward answers.
- Given a user who is not a Tailscale operator, should print the exact `tailscale serve` and operator commands instead of failing silently.
- Given an ambiguous result, no tailscale binary at a fixed absolute path, or a query past its deadline, should not bind remotely and should report the reason in status.
- Given the tailnet appearing after startup, should bind within the backoff interval while local admin and chat keep running.


**Status**: Done. Interface plus `tailscale ip -4` agreement, userspace `tailscale serve` passthrough check, backoff 5 s → 60 s and rebind, unit-tested in `bridge/linux-host.test.js`. Not testable on hosted runners: a real tailnet (the proofs use loopback).
---

## Node-native TLS identity minting

Replace `/usr/bin/openssl` in `trust-tls.js` with an in-process ECDSA P-256 self-signed certificate built from a fixed DER template, on both platforms. (Effort: S)

**Requirements**:
- Given no existing identity, should mint a certificate whose self-signature verifies and whose key matches, and write both as 0600 files under the trust writer lock.
- Given an existing `openssl`-minted identity, should load it unchanged so pins and paired devices survive.
- Given validity extending past 2049, should encode GeneralizedTime per RFC 5280.


**Status**: Done on both platforms (`bridge/identity.js`, `trust-tls.js`); existing identities load unchanged. Needs the owner's Mac app build to confirm on macOS.
---

## Linux bridge payload packaging

Teach `package-runtime.py` a directory/tarball layout for `linux-x64` and `linux-arm64`, with pinned official glibc Node and target-only native modules. (Effort: M)

**Requirements**:
- Given a target arch, should verify the pinned Node SHA-256 and include only that OS/arch's optional native packages, checked against lockfile integrity.
- Given the staged payload, should launch `--managed` under an empty HOME and stripped PATH after being moved out of the checkout, in a glibc container per arch in CI.
- Given the runtime manifest, should record the exact `pu` version and payload-relative paths, with no `../../Helpers` references.

**Status**: Implemented on `pu/linux-phone-packaging`. `package-runtime.py --layout dir --target linux-x64|linux-arm64 --pu-version <crate version> [--release-version <tag version>]` stages `<out>/purepoint-pointguard-<version>-<x86_64|aarch64>-linux-gnu/` and a reproducible `.tar.gz` + `.sha256`. Node 22.23.3 `linux-x64`/`linux-arm64` `.tar.gz` pins come from that release's `SHASUMS256.txt` (fetched over HTTPS; the darwin pins in the same file match). Foreign platform packages are removed and the target's restored from lockfile integrity, so the payload can be built on any host. Pure-function tests are in `test-package-runtime.py`; `prove-linux-runtime.mjs` is the clean-launch proof, run in CI on x64 (`macos.yml` `runtime-linux`) and per arch in `release.yml` `linux-phone`. Verified locally: a linux-arm64 payload built on a Mac passed the full proof in a `debian:bookworm-slim` container. Not done: the GPG signature on `SHASUMS256.txt` was not checked, because the host's `gpg` hung.

---

## Release assets and installer path

Publish `purepoint-pointguard-<version>-<arch>-linux-gnu.tar.gz` with checksum and signature/provenance, include `pu-point-guard-lock` in the Linux build, and add an opt-in install path. (Effort: M)

**Requirements**:
- Given an opt-in install, should verify the checksum (and the signature when the verifier is present) before unpacking into a new versioned directory and atomically switching `current`.
- Given a musl or pre-2.28 glibc host, should refuse the payload with a clear message and leave `pu`/`pu-engine` installed.
- Given a running bridge, should never modify files of the version it is executing.

**Status**: Implemented on `pu/linux-phone-packaging`; first exercised by the next `v*` release. The Linux tarballs now include the musl `pu-point-guard-lock`. `release.yml` `linux-phone` builds, installs and proves each payload. `attest` holds only `id-token`/`attestations: write`, checks out no code and signs provenance for every Linux tarball. `publish` waits for both. `install.sh` `PU_WITH_PHONE=1` / `PU_PHONE_ARCHIVE` behaves as follows:
- It refuses musl, or glibc < 2.28, and exits 3 with pu/pu-engine still installed.
- It checks the SHA-256. When `gh` is installed and signed in, it also runs `gh attestation verify --repo 2witstudios/PurePoint --signer-workflow …/release.yml`; otherwise it says provenance was not checked.
- It rejects absolute, `..`, out-of-payload or oddly named entries and any link or special file.
- It stages in `~/.pu/pointguard/.install.*` and renames with `mv -T` onto `~/.pu/pointguard/<puVersion>/`. There is no `current` link: the unit names the versioned directory (LPH-002).
- An existing complete version is left untouched, and an incomplete one is refused.

Resolved: release builds compile the tag into `pu` (`PU_RELEASE_VERSION`) and the payload records the same version, so the tag/crate mismatch no longer matters. `install.sh` trusts the manifest's `puVersion` for the directory name; `pu phone` re-checks it against itself.

---

## pu phone service commands

Add `pu phone install|uninstall|start|stop|restart|status|run`, writing a `pu-pointguard.service` user unit beside `pu-engine.service`. (Effort: M)

**Requirements**:
- Given `install --now`, should write a unit with an explicit environment that excludes `NODE_OPTIONS`, `NODE_PATH`, `PI_MOBILE_*` and `PU_*`, enable it, and warn when lingering is off.
- Given `restart` while Point Guard is busy, should refuse with the idle message instead of killing in-flight work.
- Given a payload whose manifest version differs from `pu`, should refuse to start it and tell the user how to reinstall.


**Status**: Done (`crates/pu-cli/src/phone.rs`, `commands/phone.rs`). The unit adds `LockPersonality`, `RestrictRealtime`, `RestrictSUIDSGID` and `SystemCallArchitectures=native`. `pu` records the release version (`PU_RELEASE_VERSION`) so the payload lockstep is exact. Pruning keeps newer versions, the unit's payload and install staging. CI (`runtime-linux`) runs install, readiness, restart (identity kept), idle stop (not resurrected) and uninstall (trust kept) under a real systemd user manager.
---

## Terminal pairing

Implement `pu phone pair` with Unicode QR rendering in Rust, polling, revoke-on-exit, and a `--text` paste fallback (no QR image on disk). (Effort: M)

**Requirements**:
- Given a TTY of at least the needed size, should render a QR that scans on both light and dark terminal themes.
- Given Ctrl-C, SIGHUP, SIGTERM or normal exit before the enrollment is consumed, should revoke it, and report "may remain valid until <time>" when the revoke is unconfirmed.
- Given a successful enrollment, should print the device name and its default grants.


**Status**: Done. Signal handlers exist before the enrollment; the success message uses the durable `deviceId` from `pairing.status`. The proof drives `pu phone pair` (paste code, enrollment, exit 0) and SIGHUP revocation.
---

## Device administration commands

Add `pu phone devices|grant|rename|revoke` over the admin transport, with a new `devices.rename` admin op. (Effort: S)

**Requirements**:
- Given a device argument, should resolve by exact name or unique id prefix and fail on ambiguity.
- Given a grant upgrade to chat `send` or terminals `control`, should state plainly what the device gains on this host before applying.
- Given `--json`, should emit stable fields without any credential or hash material.


**Status**: Done, with `devices.rename`. The proof drives `pu phone grant` and `revoke` with grants checked over the pinned WebSocket.
---

## Headless provider setup

Add `pu phone login|model|cwd` driving `auth.*`, `model.select` and `runtime.configure`. (Effort: M)

**Requirements**:
- Given an OAuth flow that emits a URL or device code, should print it for use on another device and accept `manual_code` pastes.
- Given `--api-key-stdin` or a secret prompt, should read without echo and never put the key in argv, logs or the state directory.
- Given a provider flow that needs a host-local browser callback with no manual fallback, should say so and suggest an API key or SSH port forward.


**Status**: Done. URLs, device codes and links printed; no-echo secrets and `--api-key-stdin`; attempts canceled on exit; `pu phone model` with no argument lists models. Live provider sign-in needs real credentials and is not exercised in CI.
---

## iOS host-neutral pairing

Make iOS copy host-neutral, read `hostKind`/`hostLabel` from `/pair/verify`, and accept a `purepoint-pair:` clipboard enrollment. (Effort: M, owner-built in Xcode)

**Status**: Implemented on `pu/linux-phone-ios`; awaiting owner Xcode build, XCTest run and device check. Paste path: Connections → Pair a computer → Paste code (or Enter code manually). Description fields persist on the per-host `SavedMachine` metadata (`pi.machines.v1`), not the Keychain trust record.

**Requirements**:
- Given a Linux host, should label it with its host label, show a host-appropriate re-pair hint, and keep it distinct from a Mac with the same label by hostId.
- Given a pasted enrollment string, should apply exactly the scanned-QR validation, including the pin, expiry and endpoint allow-list.
- Given an older bridge without `hostKind`, should behave as today.

---

## Connected machines through a Linux host

Expose the Linux host's registered SSH machines to the phone under the same device grants as on the Mac. (Effort: S)

**Requirements**:
- Given a registered machine with a healthy forward, should list its projects and agents scoped by machine ID, applying the device's current grants.
- Given an unreachable or protocol-incompatible machine, should show it as unavailable without affecting the host or other machines, and never auto-start its daemon.


**Status**: Done with no code change: `daemon.js` and `terminals.js` are platform-neutral and read `~/.pu/machines.json`; unavailable machines show as unavailable. Copy is host-neutral.
---

## End-to-end Linux verification

Add a CI job that runs the packaged Linux bridge in a container, enrolls with the existing pinned-TLS fixture, and exercises grants and revocation over the real listener. (Effort: M)

**Requirements**:
- Given the packaged payload in a clean container, should complete enrollment, verify the pin, enforce chat/terminal/workspace grants and revoke while other devices stay connected.
- Given a restart through `pu phone restart`, should preserve host identity and device trust and invalidate outstanding enrollments.

**Status**: Seed done on `pu/linux-phone-packaging`. `macos.yml` `runtime-linux` packages the x64 payload and runs `prove-linux-runtime.mjs` twice: relocated, and installed by `install.sh` into a throwaway HOME. Both runs use an empty HOME and a toolless PATH. The proof covers:
- private state and Unix-socket admin modes
- `pu phone status` against the running bridge
- single-use enrollment over the pinned-TLS listener, with the certificate SHA-256 checked against the QR
- device credentials and the chat token rejected as owner authority
- the state-lock collision
- `pu phone devices`/`revoke`
- host identity and the revoked device persisting across a restart

It also checks the musl refusal in Alpine.

Since extended: the proof also enrolls a second device, sets grants with `pu phone grant`, checks derived chat levels and refusals over the pinned WebSocket, revokes one device while the other stays connected, applies a live downgrade, drives `pu phone pair` (paste and SIGHUP revoke), and checks grants persist across restart (passed in `debian:bookworm-slim` on arm64). `runtime-linux` runs the unit under a real systemd user manager (the runner VM, with lingering enabled).

Remaining: a real tailnet address can't exist on hosted runners; discovery is unit-tested and the listener is proven on loopback.

---
