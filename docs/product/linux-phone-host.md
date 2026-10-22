# Linux phone host

**Maturity: SPECIFIED** | ID Prefix: LPH | Dependencies: `product/point-guard-setup.md`, `product/machines.md`, `architecture/distribution.md`, `architecture/desktop-app-integration.md`, `product/cli.md`

## Purpose

Let a user pair an iPhone or iPad directly with a headless Linux machine running PurePoint, such as a cloud VM or a home server, with no Mac involved. Today a phone reaches a Linux daemon only through a Mac that stays on and forwards over SSH (`product/machines.md`, `apps/purepoint-mobile/README.md:3`). This spec runs the existing Point Guard bridge headless on Linux, shows the enrollment QR in a terminal, and keeps the trust model the Mac already uses: pinned TLS, a one-time QR, per-device credentials, checks on every message, and revocation.

In scope: packaging, lifecycle, terminal pairing and device administration, headless provider setup, iOS changes for non-Mac hosts, and making the security model match the Mac's. Out of scope: Windows, multi-user hosts, and agent sandboxing and permissions (the user's responsibility). Machines the Linux host reaches over SSH are in scope, exposed as on the Mac (LPH-012).

## Conceptual Model

```
iPhone/iPad ──pinned WSS (Tailscale)──▶ Linux host
                                         ├─ pu-pointguard (systemd user unit)
                                         │    ├─ bundled Node + bridge (same code as the Mac)
                                         │    ├─ Pi child (RPC), native ~/.pi/agent auth/sessions
                                         │    ├─ remote listener: <tailnet IP>:8787, pinned TLS
                                         │    ├─ local desktop-chat listener: loopback only
                                         │    └─ owner admin: Unix socket in a private 0700 dir
                                         └─ pu-engine (existing systemd user unit) ◀── bridge via ~/.pu/daemon.sock
owner shell (SSH) ── pu phone … ──▶ owner admin socket
```

- **Host**: a machine running a Point Guard bridge that a phone can pair with. A Mac host is owned by the Mac app; a **Linux host** is owned by a systemd user unit and administered with `pu phone`.
- **Owner**: on a Mac, the app process holding the admin token. On Linux, the Unix user that runs the unit. Any process running as that uid counts as the owner. Agents run as that uid, so how far to trust them is up to the user (out of scope, see Purpose).
- **Bridge payload**: one versioned directory holding the bundled Node, `bridge/`, production `node_modules`, `support/`, `docs/`, the `pu-point-guard-lock` helper and `runtime-manifest.json`. It ships in lockstep with `pu`/`pu-engine`.
- **Device grant**: per paired device, `terminals ∈ {off, view, control}` and `workspaceManagement ∈ {false, true}`. Point Guard chat access is derived from those grants (PGSET-012, PR #264): none for Off, read-only for View or Control without workspace management, full only for Control plus workspace management. Every bridge operation and every broadcast is checked against the device's current grant.
- **Enrollment**: an in-memory, single-use, short-lived token plus the host certificate pin, encoded in the QR. This is unchanged from PGSET-004.

## Current coupling inventory

Everything in the bridge and runtime packaging that assumes macOS or the Mac app. "Portable" means the code already works on Linux and only its caller changes.

### Bridge (`apps/purepoint-mobile/bridge/`)

| Location | Assumption | Linux treatment |
|---|---|---|
| `runtime.js:59-61`, `pair.js:7-9` | Default state dir `~/Library/Application Support/PurePoint/PointGuard` | Platform default: `${XDG_STATE_HOME:-~/.local/state}/purepoint/point-guard` (LPH-004); the unit always sets `POINT_GUARD_STATE_DIR` explicitly |
| `runtime.js:145`, `app-lifetime.js:2-26` | Lifetime tied to the Mac app through the stdin EOF channel (`POINT_GUARD_APP_LIFETIME=stdin`) | Not set under systemd, so the watcher is a no-op (`app-lifetime.js:3`). SIGTERM drain (`runtime.js:90-101`) is the stop path |
| `runtime.js:164-169` | `lock_busy` retry window only when launched by the app (overlapping replacement) | systemd serializes stop then start; extend the retry to an explicit `POINT_GUARD_SUPERVISOR=systemd` so a quick restart doesn't fail |
| `runtime.js:177-182` | `POINT_GUARD_PU_PATH` must point to the bundled pu ("Reinstall the complete PurePoint app") | Unit sets it to `~/.pu/bin/pu`; error text becomes host-neutral |
| `runtime.js:193-196`, `setup.js:24-47` | Admin and desktop-chat tokens as private files read by the Mac app (`PointGuardServiceModel.swift:104-105`) | Admin moves to a Unix socket (LPH-006); the desktop-chat listener stays for a future local client but is not required on Linux |
| `runtime.js:294-309` | `PI_MOBILE_HOST` chosen by the Mac app (`PointGuardServiceModel.swift:86-89`) before launch; with no address, the remote listener is skipped until restart (`remote-listener.js:2-16`) | Linux discovers the address in the bridge or the unit's start step and rebinds when Tailscale appears later (LPH-008) |
| `runtime.js:363-462`, `admin.js:29-102` | Owner admin is loopback HTTP plus a bearer token on an ephemeral port, published through `admin.json` | Linux transport: Unix socket (LPH-006). The dispatch table is reused unchanged, with `devices.rename` added |
| `runtime.js:423-427`, `pairing.js:47-49`, `pair.js:46-48` | User-facing copy says "this Mac", "Your Mac", "Open PurePoint … Connect phone" | Host-neutral copy, with a host-kind-specific recovery hint |
| `pairing.js:7-10, 41-58`, `pair.js:1-50` | QR is delivered as an SVG HTML page at `~/.config/pi-mobile/pairing.html` (developer path) | Replaced on Linux by `pu phone pair` terminal rendering (LPH-007); no QR file is written to disk by default |
| `trust-tls.js:53-72` | Identity minted with absolute `/usr/bin/openssl` | Replaced on both platforms by in-process minting with Node's built-in crypto (LPH-010) |
| `trust-tls.js:12-24`, `runtime-state.js:14-30`, `native-provider.js:10-26` | uid/mode/symlink checks via `process.getuid` | Portable |
| `lock-helper.js:29-73`, `crates/pu-cli/src/bin/point_guard_lock.rs:71` | Kernel `flock` held by the `pu-point-guard-lock` helper binary; helper env `PATH=/usr/bin:/bin` | Portable (Linux `flock` keeps the same open-file-description semantics). The helper is missing from the Linux tarball (`release.yml:302-317` builds and copies only `pu` and `pu-engine`) |
| `endpoints.js:2-9`, `network.js:26-29` | Remote bind limited to tailnet CGNAT/ULA ranges or loopback | Portable; kept (LPH-008) |
| `native-provider.js:29-31` | Native Pi dir `~/.pi/agent` | Portable; same as the Pi CLI on Linux (DIST-002) |
| `daemon.js:102-103` | `~/.pu` daemon home and `machines.json` | Portable |
| `setup.js:102-128` | Workflow/support packages resolved relative to the bridge root | Portable inside the bridge payload |
| `main.js:7-12` | `--managed` entry point | Portable; same entry point under systemd |

### Mac-app-side ownership that Linux must replace

| Location | Responsibility | Linux owner |
|---|---|---|
| `PointGuardServiceModel.swift:57-58` | State dir choice | Unit env |
| `PointGuardServiceModel.swift:74-91` | Env scrubbing (`NODE_OPTIONS`, `NODE_PATH`, `PI_MOBILE_*`, `PU_*`), fixed `PATH`, cwd = home | Unit sets every bridge variable explicitly with `Environment=` and removes anything the user manager imported with `UnsetEnvironment=` (`NODE_OPTIONS`, `NODE_PATH`, `LD_PRELOAD`, `PI_MOBILE_*`, `PU_*`, owner bootstrap variables). `pu phone run` starts from an empty environment |
| `PointGuardServiceModel.swift:93-99` | Owned child, lifetime pipe, readiness wait | systemd plus `pu phone status` readiness read over admin |
| `PointGuardTailnet.swift:5-11, 37-42` | Tailnet IPv4 on a `utun*` interface, confirmed with `tailscale ip -4` from fixed macOS paths | Linux discovery (LPH-008) |
| Connect phone sheet (`PointGuardSetupView.swift`) | QR display, close revokes, device list, one Access menu per device (maps onto the terminal and workspace grants) | `pu phone pair`, `pu phone devices`, `pu phone grant` |
| Provider setup UI | OAuth/API key over `auth.*` admin ops | `pu phone login` (LPH-009) |

### Runtime packaging (`apps/purepoint-mobile/runtime/`)

| Location | Assumption | Linux treatment |
|---|---|---|
| `node-release.json:3-12` | Pins only `darwin-arm64`/`darwin-x64` archives | Add `linux-arm64`/`linux-x64` (official glibc builds) with SHA-256 pins (LPH-001) |
| `package-runtime.py:43-46` (`complete_darwin_graph`) | Restores only Darwin optional natives from the lockfile | Generalize to a target OS. The lockfile already records `@esbuild/linux-*` entries with integrity |
| `package-runtime.py:97-101, 106-118` | Output must be a `.app` outside `/Applications`; checks for `_CodeSignature` | New `--layout dir` target producing a plain directory and tarball |
| `package-runtime.py:129-132` | `/usr/bin/lipo` universal Node | One Node binary per arch tarball |
| `package-runtime.py:145-151` | Deletes `pi-tui` `native/linux` prebuilds | Keep only the target OS/arch prebuild |
| `package-runtime.py:163-170` | Manifest paths `../../Helpers/…` (bundle-relative) | Payload-relative `bin/node`, `bin/point-guard-lock`; `pu` resolved from `~/.pu/bin` and version-checked |
| `sign-runtime.py:1-35`, `node-entitlements.plist` | `codesign`, hardened runtime, Node JIT entitlement | Not applicable. Replaced by checksum plus a release signature/provenance attestation (LPH-002) |

## Open Questions

! [LPH-001] Ship official nodejs.org glibc Node builds only (glibc ≥ 2.28); musl/Alpine hosts are refused with a clear message — owner chose the best long-term option: PurePoint never maintains its own Node build or depends on an experimental source, and support follows automatically if Node.js publishes official musl builds.
! [LPH-002] A separate opt-in release asset `purepoint-pointguard-<version>-<arch>-linux-gnu.tar.gz` with `.sha256` and a release signature/provenance attestation, unpacked by `install.sh` (`PU_WITH_PHONE=1`) into a new `~/.pu/pointguard/<version>/`; `pu phone` runs only the payload whose manifest `puVersion` equals its own — servers that never pair a phone don't pay for Node, and a running version is never modified.
! [LPH-003] A hardened systemd user unit `pu-pointguard.service` written by `pu phone install`, pointing at the exact versioned payload; `pu phone restart` stops only when idle, rewrites the unit for the new version, starts it and prunes payloads nothing runs; `pu phone run` covers hosts without systemd — reuses the proven `pu daemon install` pattern, and all trust and session state lives outside the payload.
! [LPH-004] `${XDG_STATE_HOME:-~/.local/state}/purepoint/point-guard/`, 0700 directories and 0600 files verified with `lstat` — keeps remote-access trust out of `~/.pu`, which has a broader writer set.
! [LPH-005] The command group is `pu phone` — owner decision; it names the user's goal, and the unit can still host Point Guard chat for other clients later.
! [LPH-006] A Unix socket in a directory recreated at 0700 on every start, socket set to 0600 after bind regardless of umask, bearer token still required; `pu` checks directory, socket and token ownership and modes before sending the token — no TCP surface, and other uids are excluded by the filesystem; the owner is the Unix user.
! [LPH-007] `pu phone pair` renders the QR in Rust (`qrcode` crate, explicit black-on-white, quiet zone) and falls back to a `purepoint-pair:` paste string when stdout isn't a terminal or is too narrow; it revokes the enrollment on any exit except success — keeps QR code out of the long-lived listener process; no QR image is written to disk.
! [LPH-008] Kernel-TUN Tailscale binds the confirmed tailnet IP directly; userspace-networking Tailscale is supported in the first slice through `tailscale serve` TCP passthrough to a loopback listener; no Tailscale means no remote listener — owner decision to cover container and VM hosts from the start, while TLS stays end to end and pinned in both modes.
! [LPH-009] `pu phone login` drives the existing `auth.*` admin flows (URLs and device codes printed for another device, no-echo secret prompts, `--api-key-stdin`) — one owner channel that reuses Pi's catalog and stale-commit guards.
! [LPH-010] Mint new TLS identities in-process with Node's built-in crypto (ECDSA P-256, fixed-template self-signed certificate, parsed and signature-verified before use), on both Linux and the Mac — the best long-term option the owner asked for: no OS-tool dependency, no new native binary to build, sign and ship, one code path on both platforms; existing identities and pins stay valid, so nobody re-pairs.
! [LPH-011] Host-neutral copy, optional `hostKind`/`hostLabel` in the authenticated `/pair/verify` response (never in the QR), and paste enrollment with the same validation as a scan — older apps and bridges keep working, and identity data arrives only over the pinned channel.
! [LPH-012] A Linux host exposes the machines it reaches over SSH to the phone, the same way a Mac host does, under the same device grants — owner decision; it matches the Mac, where the Agents surface already spans connected machines.
! [LPH-013] Adopt PGSET-012 from PR #264: chat access is derived from the existing grants (Off → none, View or Control without workspace management → read-only, Control plus workspace management → full), checked per message and per broadcast — no new field, migration or admin op, and since chat can run `pu`, full chat requires the broadest grant. Linux work depends on #264 landing first.
! [LPH-014] One bridge with a small platform seam (`platform.js`, `tailnet.js`, `remote.js`); only packaging and the owner UI fork — avoids two diverging trust implementations.

## Research Notes

**Researched: 2026-10-21** against `main` at 2206129.

#### [LPH-001] Node build and libc floor

Option A: official nodejs.org `linux-x64`/`linux-arm64` tarballs (glibc), the same 26.12.1 that the Mac pins.
- Pro: the same upstream, version and SHA256SUMS flow as `node-release.json`; covers Debian/Ubuntu/Fedora/RHEL 8+, Amazon Linux 2023 and Raspberry Pi OS 64-bit.
- Con: needs glibc ≥ 2.28 (Node 26's Linux build floor, unchanged from 22). Alpine/musl hosts are unsupported, unlike the static musl `pu` binaries (DIST-004).

Option B: unofficial-builds musl Node.
- Pro: runs on Alpine.
- Con: x64 only for this line, "experimental" tier, a different supply-chain source.

Option C: rely on the system Node.
- Con: breaks PGSET-001's "no external Node" requirement and the pinned-runtime guarantees.

Decided (LPH-001): A. Install detects musl or old glibc and refuses with a clear message. Musl support depends on Node.js publishing official musl builds; PurePoint will not build or vendor its own Node.

#### [LPH-002] Packaging and distribution

Option A: a separate release asset `purepoint-pointguard-<version>-<arch>-linux-gnu.tar.gz` plus `.sha256`, installed by `install.sh` only when asked (`PU_WITH_PHONE=1`, or `pu phone install` fetching the matching version). It unpacks to a new `~/.pu/pointguard/<version>/`; the unit names that exact directory, so there is no shared `current` link to race.
- Pro: server users who never pair a phone don't download about 40 MB of Node and modules. Versioned dirs let a running bridge keep its files while a new version lands (the same no-overwrite principle as DIST-004's rename install).
- Con: one more asset per arch; install.sh grows an optional path.

Option B: put the payload into the existing `pu` tarball.
- Pro: one download.
- Con: every server pays the size, and a glibc payload sits inside an otherwise static musl tarball that claims "runs on any distribution".

Integrity: the same SHA-256 check as `install.sh:51-58`, plus a release signature or build-provenance attestation that the installer verifies when its tool is present. The bridge payload is code that holds remote-access trust, so it needs at least the guarantees of the `pu` binaries. Lockstep: `runtime-manifest.json` records the `pu` version it was built with. `pu phone` refuses to start or administer a bridge whose manifest version differs from its own, and says to run `pu phone install`.

Decided (LPH-002): A.

#### [LPH-003] Lifecycle

Option A: a dedicated systemd user unit `pu-pointguard.service` written by `pu phone install [--now]`, mirroring `pu daemon install` (`crates/pu-cli/src/service.rs`). `Wants=`/`After=pu-engine.service`, `Restart=on-failure`, `ExecStart=<current>/bin/node <current>/bridge/main.js --managed`, an explicit environment, and `KillSignal=SIGTERM` with a `TimeoutStopSec` longer than the bridge's drain. Lingering is reported the same way `pu daemon install` reports it.
- Pro: survives logout and reboot with lingering, reuses the proven service code, and journald collects the (secret-free) logs.
- Con: systemd-only. Non-systemd hosts run `pu phone run` in the foreground under their own supervisor.

Option B: the daemon (`pu-engine`) spawns and owns the bridge.
- Pro: one service.
- Con: Node supply-chain code gets coupled to the daemon's process tree, and DIST-003 "never adopt unknown processes" becomes harder to reason about.

Upgrade: install the new version dir. `pu phone restart` calls admin `runtime.stop` (idle-only, `runtime.js:417-422`), waits for the unit to become inactive, rewrites the unit for the new version dir, starts it, and removes payload dirs nothing runs. Trust, TLS identity, native sessions and the selected provider/cwd live in the state dir and Pi dir, so they survive the switch (PGSET-006). Outstanding enrollments are invalidated by the restart, as designed. A forced `systemctl --user restart` while busy follows the existing SIGTERM drain and no-replay rules. `runtime.stop` exits 0, so `Restart=on-failure` does not resurrect a deliberate stop.

Decided (LPH-003): A, with `pu phone run` for foreground and non-systemd use. The unit also sets `UMask=0077`, `NoNewPrivileges=yes` and `UnsetEnvironment=` for inherited variables.

#### [LPH-004] State location and modes

Option A: `${XDG_STATE_HOME:-$HOME/.local/state}/purepoint/point-guard/` (trust, TLS identity, runtime.json, admin socket dir).
- Pro: separate from `~/.pu`, which holds daemon metadata shared with project tooling; follows the XDG convention for state that is not config.

Option B: `~/.pu/point-guard/`.
- Pro: one place to look.
- Con: mixes remote-access trust into a directory with a broader writer set.

Requirements either way: every directory is created 0700 and verified by `lstat` (not a symlink, owned by the euid, no group/other bits) before use, and files are 0600, as on the Mac (`runtime-state.js`, `trust-tls.js`). Native Pi keeps its existing `~/.pi/agent` rules (PGSET: existing native Pi directory permissions). The unit file itself is 0644 and contains no secrets.

Decided (LPH-004): A.

#### [LPH-005] CLI surface

Decided (LPH-005), one `pu phone` group:

| Command | Admin op |
|---|---|
| `pu phone install [--now]` / `uninstall` | unit + payload |
| `pu phone start` / `stop` / `restart` / `status` | systemd + `status` |
| `pu phone run` | foreground `--managed` |
| `pu phone login [provider] [--api-key-stdin]`, `pu phone model <provider>/<id>`, `pu phone cwd <path>` | `providers`, `auth.*`, `model.select`, `runtime.configure` |
| `pu phone pair [--ttl 120] [--text]` | `pairing.create` → poll `pairing.status` → `pairing.revoke` on exit |
| `pu phone devices [--json]` | `devices.list` |
| `pu phone grant <device> [--terminals off\|view\|control] [--workspace on\|off]` | `devices.terminals`, `devices.workspaceManagement` (downgrades applied first; chat level is derived and shown) |
| `pu phone rename <device> <name>` | `devices.rename` (new) |
| `pu phone revoke <device>` | `devices.revoke` (re-pairing replaces a rotated credential) |

Devices are addressed by name or a unique device-id prefix, and ambiguity is an error. Every command works over plain SSH with no TTY features except `pair` (LPH-007).

#### [LPH-006] Owner admin channel

There is no app on Linux to hold the admin token or read `admin.json`.

Option A: keep loopback HTTP and the bearer `admin-token` file (Mac parity); `pu` reads both.
- Pro: zero bridge change.
- Con: a TCP listener reachable by every local uid on the host, guarded only by a bearer secret.

Option B: an owner Unix socket at `<state>/admin/admin.sock`. The `admin/` dir is created fresh at each start with mode 0700 and verified with `lstat`, and the socket is set to 0600 with `chmod` right after bind, so its effective mode never depends on umask. The bearer token stays as a second factor and is read from its 0600 file by `pu`. HTTP framing, the `Origin` refusal, size and time limits and the dispatch table (`admin.js`) are reused over the socket.
- Pro: filesystem permissions keep other uids out entirely, and there is no TCP surface. On Linux, `connect(2)` needs write permission on the socket inode, and the 0700 parent covers the window before `chmod`.
- Con: Node has no built-in `SO_PEERCRED`. A peer-uid check needs a small helper (the lock-helper pattern), and the directory check makes it defense in depth only.

Owner authority itself: on Linux the owner is the Unix user. A same-uid process can always reach owner admin, just as it can reach `~/.pu/daemon.sock`. Trust-raising operations (`pairing.create`, grant upgrades) are logged to journald with device id and grant (never secrets). `pu phone devices` shows each device's grants, derived chat level and creation time, so the owner can audit them.

Decided (LPH-006): B.

#### [LPH-007] Terminal QR

Payload size: the QR v1 JSON (`pairing.js:30-38`) is about 300 bytes. At error correction M that is roughly version 11-13 (61-69 modules plus a 4-module quiet zone). Rendered with Unicode half-blocks (two modules per character row), that is about 77 columns × 39 rows, which fits an 80×40 terminal. Smaller terminals get a resize hint instead of a broken code.

Option A: render in `pu` (Rust) with the `qrcode` crate's Unicode renderer, using explicit ANSI white-background/black-foreground SGR codes so dark themes don't invert it.
- Pro: keeps the QR library out of the bridge listener process (see LPH-014); `pu phone pair` controls TTY detection, width and cleanup.
- Con: adds a crate, which has to pass `cargo deny`.

Option B: the bridge renders with its existing `qrcode@1.5.4` (`type: "terminal"`) and returns text.
- Pro: no new dependency.
- Con: rendering happens inside the long-lived listener process.

Fallbacks when the terminal can't show a scannable code (not a TTY, too narrow, `NO_COLOR`/`TERM=dumb`, or the user asks with `--png`/`--text`):
- `--png <path>`: write a 0600 PNG in a 0700 dir, deleted once the enrollment is consumed, expires or is revoked.
- `--text`: print the enrollment as a one-line `purepoint-pair:` string the iOS app accepts from the clipboard. This needs a new iOS paste path (LPH-011).

TTL defaults to 120 s with a maximum of 300 s (unchanged). Closing the sheet revokes the enrollment: `pu phone pair` stays in the foreground, polls `pairing.status`, and on success prints the new device name and its default grants. On Ctrl-C, SIGHUP (SSH drop), SIGTERM or normal exit it calls `pairing.revoke`. If the revoke can't be confirmed, it prints "enrollment may remain valid until <time>", matching the Mac sheet's uncertain state.

Decided (LPH-007): A, with the `--text` paste fallback. The `--png` fallback is dropped: it would write enrollment material to disk.

#### [LPH-008] Endpoint selection

On Linux, Tailscale runs either with a kernel TUN interface (`tailscale0`) or in userspace-networking mode (containers, some VMs). In userspace mode no interface holds the 100.x address, so bind fails with `EADDRNOTAVAIL`.

Option A (port of the Mac logic): scan the interfaces for an up interface holding a 100.64.0.0/10 address (prefer `tailscale0`), confirm it with `tailscale ip -4` from fixed absolute paths (`/usr/bin/tailscale`, `/usr/sbin/tailscale`, `/usr/local/bin/tailscale`) using a bounded, scrubbed environment as `PointGuardTailnet.swift` does, and bind the remote listener to exactly that address.
- Pro: the same guarantees as the Mac. Two independent sources must agree.
- Con: no userspace-networking support.

Option B: in addition to A, support userspace mode by binding loopback and having the user run `tailscale serve --tcp 8787 tcp://127.0.0.1:<port>`. TLS stays end to end (TCP passthrough), and the phone still pins the bridge certificate.
- Pro: works in containers.
- Con: the listener sees every connection as loopback, so peer-address checks can't tell the phone from local processes in that mode. Authorization still rests on pinned TLS and device credentials.

Serve-mode design: the bridge detects userspace mode when `tailscale ip -4` reports an address that no interface holds. It then binds the remote (pinned-TLS, device-trust) listener on `127.0.0.1:<port>` and advertises `wss://<tailnet IP>:<port>/v1` in the enrollment. Port 8787 is the default, and the advertised port always equals the serve port. `pu phone install` applies `tailscale serve --bg --tcp <port> tcp://127.0.0.1:<port>` when the user is a Tailscale operator; otherwise it prints that command, plus `sudo tailscale set --operator=$USER`, for the user to run. `status` shows `remote: tailscale serve (userspace)` or the missing step. The loopback listener is separate from the loopback desktop-chat listener and accepts only device-trust TLS connections.

With no Tailscale, the remote listener is not started. Local admin, provider setup and native chat keep working (`remote-listener.js`), and `pu phone pair` reports "no tailnet address; connect Tailscale (`tailscale up`) and run `pu phone restart`". Arbitrary LAN or public binds stay refused (`endpoints.js`, `network.js:26-29`). Late Tailscale: unlike the Mac, a headless boot often starts the user unit before `tailscaled` is up. The bridge retries discovery and binding with backoff (for example 5 s doubling to 60 s), and `status` shows `remote: waiting for tailnet`. If the tailnet address changes, the old listener is closed, the new address is bound, and saved phones need a new pairing only if the endpoint changed (the pin and hostId are unchanged).

Decided (LPH-008): A plus late-bind retry, and B in the first slice.

#### [LPH-009] Headless provider setup

`ProviderSetup` already exposes Pi's flows as data: `notify` events (auth URLs, codes) and `prompt` with `text`/`secret`/`select`/`manual_code` (`provider.js:79-100, 131-133`). A terminal client can drive them.

Option A: `pu phone login` runs `auth.start`/`auth.status`/`auth.respond` over admin. It prints auth URLs and device codes for the user to open on any device, reads `secret` prompts with echo off, and accepts `manual_code` pastes. `--api-key-stdin` reads a key from a pipe so it never appears in argv or shell history.
- Pro: one owner channel; uses Pi's existing catalog and stale-commit guards (PGSET-003).
- Con: OAuth flows that only finish through a browser redirect to a localhost callback on the host need either a `manual_code` fallback from Pi or an SSH port forward. The CLI lists only the methods the catalog supports headless and labels the rest "needs port-forward".

Option B: run the bundled Pi CLI's own `/login` against the same `~/.pi/agent` (DIST-002 shares native storage).
- Pro: zero new code.
- Con: bypasses the bridge's idle and supersede guards while the bridge is running.

Credentials stay in `~/.pi/agent/auth.json` (0600, existing checks in `native-provider.js:28-44`). No credential passes through the bridge state dir, journald, argv or the QR.

Decided (LPH-009): A.

#### [LPH-010] TLS identity minting

Option A: keep `/usr/bin/openssl` and check for it in `pu phone install` and `status`. Present on mainstream distros; missing from distroless and some minimal images.

Option B: a private Rust helper using `rcgen`. No OS tool, but `rcgen` pulls in `ring` or `aws-lc-rs` (C and assembly) for the static musl `pu` build, and the Mac bundle gains another binary to sign.

Option C: mint in-process with Node's built-in crypto. Node can generate keys and sign but has no X.509 builder, so the bridge encodes one fixed-template certificate in DER (version 3, random positive serial, `CN=Point Guard` as subject and issuer, ten-year validity, basic constraints CA, subject and authority key identifiers) and signs it with ECDSA P-256/SHA-256. Only the key, serial and dates vary, and no caller input reaches the encoder. The certificate is parsed with `X509Certificate` and its self-signature checked before it is written. A prototype passed `openssl x509 -text` and `openssl verify`, and the phone's pin check accepts any leaf because it uses a basic X.509 policy anchored on the exact pinned certificate (`PairingSecret.swift:181-196`).

Decided (LPH-010): C on both platforms. Files are written 0600 in a `mkdtemp` (0700) directory under the trust writer lock, then renamed into place. Existing `openssl`-minted RSA identities are loaded unchanged, so pins and paired devices survive.

#### [LPH-011] iOS changes

- Copy: "Your Macs", "Scan Mac QR code" and "Mac trust" strings (`SetupViews.swift:82`, `PairingSecret.swift:29-83`) become host-neutral ("Your computers").
- Host kind: add optional `hostKind: "mac" | "linux"` and `hostLabel` (the host's short hostname) to the authenticated `/pair/verify` response (`network.js:74-84`). Keep it out of the QR, so older apps that strictly validate QR fields aren't affected. The phone uses it for the default name and icon, and to word recovery hints ("run `pu phone pair` on <label>" instead of "open Connect phone on your Mac").
- Multiple hosts: the phone already stores trust per host and endpoint and offers a picker (`README.md:41`). The work is checking that a Mac and a Linux host on the same tailnet with matching labels stay distinct (by hostId) and that per-host names persist.
- Capabilities: the existing capability check covers features. A Linux host advertises the same capability set as a Mac, including machines it reaches over SSH (LPH-012).
- Paste enrollment: accept a `purepoint-pair:` clipboard string as a QR-scan alternative (LPH-007), with the same validation as a scanned code. The phone's endpoint allow-list (`ChatDomain.swift:153`) is unchanged.

#### [LPH-012] Machines reached from the Linux host

`daemon.js:102-110` already reads `~/.pu/machines.json`, so a Linux host with registered machines offers them to the phone through its own SSH forwards with no new trust machinery.

Decided (LPH-012): expose them, as on the Mac. The same per-device grants (terminals Off/View/Control, workspace management, chat) apply to every machine. Registered machine IDs scope every target, and the phone never uses SSH. The Linux host's forwards must stay up for remote machines to be reachable. A machine whose daemon is unreachable or protocol-incompatible appears as unavailable, never as a guessed state. The `pu machine` requirements (MACH-003: no remote auto-start) still apply.

#### [LPH-013] Chat as a device grant

Point Guard can run `pu`, so sending chat amounts to driving the host's workspace. Grants must therefore cover every bridge operation, not only terminals and workspaces.

PR #264 (PGSET-012) already implements this on the shared bridge without a new field: chat level is derived from the existing grants.

| Grants | Point Guard chat |
|---|---|
| terminals Off | none: chat ops refused, no snapshots or editor offers |
| View, or Control without workspace management | read-only: `sync`, `sessions`, `history` and broadcasts |
| Control plus workspace management | full |

Decided (LPH-013): adopt it unchanged. `pu phone devices` and `pu phone grant` print the derived level, and `grant` warns when a change enables full chat. Linux implementation depends on #264 merging first. A further layer (running chat turns from narrower devices with a `pu` that cannot mutate the daemon) remains a possible later hardening, not a Linux blocker.

#### [LPH-014] Shared vs forked

Shared without a fork: `network.js`, `trust.js`, `trust-tls.js`, `controller.js`, `core.js`, `rpc.js`, `terminals.js`, `workspace.js`, `daemon.js`, `provider.js`, `native-provider.js`, `lock-helper.js`, `runtime-state.js`, `pairing.js` (payload validation) and the Rust lock helper.

Platform seam: `platform.js` (state dir, host kind and label, admin transport, copy), `tailnet.js` (discovery and serve-passthrough check) and `remote.js` (late-bind listener manager). `runtime.js` takes these through parameters, not `process.platform` checks spread through the code.

Decided (LPH-014): one bridge with this seam.

Forked: packaging (`package-runtime.py` gains a dir/tarball layout; `sign-runtime.py` stays Mac-only) and the owner UI (SwiftUI sheet vs `pu phone`).

Keeping the listener process minimal benefits both platforms. Rendering the QR in `pu`, and keeping provider SDK work out of the listener where practical, shrinks the code a network attacker can reach.

## Requirements

- Given a Linux host with phone access installed, `pu phone install --now` should write `pu-pointguard.service` (0644, no secrets) naming the exact versioned payload, enable and start it, and warn when lingering is off.
- Given a payload whose manifest `puVersion` differs from `pu`'s release version, every `pu phone` command that runs or writes the unit should refuse and name the reinstall command.
- Given `pu phone stop` or `restart` while Point Guard is busy, should refuse with the idle message unless `--force`; an idle stop should leave the unit inactive (exit 0, not restarted).
- Given `pu phone restart` after an upgrade, should move the unit to the new payload, keep host identity and device trust, and prune only payload versions older than the one kept (never newer ones, the unit's payload or install staging directories).
- Given `pu phone pair` on a terminal wide enough, should render a black-on-white QR with a quiet zone; otherwise (or with `--text`) should print a `purepoint-pair:` paste code. On SIGINT, SIGTERM, SIGHUP or any exit before use it should revoke the code, and on success print the enrolled device (identified by the durable device id) and its grants.
- Given `pu phone grant`, should apply downgrades before upgrades, print the derived chat level, and warn when a change can enable full chat.
- Given `pu phone devices --json`, should emit `deviceId`, `name`, `createdAt`, `revokedAt`, `terminals`, `workspaceManagement` and `chat`, and never credential or hash material.
- Given `pu phone login`, should print auth URLs, device codes and links for use on another device, read secrets without echo or from `--api-key-stdin`, cancel the attempt on any exit except success, and say that a restart applies it.
- Given a phone on a Linux host, the authenticated `/pair/verify` response should include `hostKind: "linux"` and a `hostLabel` of `[A-Za-z0-9-]{1,63}`, used by the phone for display only.
- Given the iOS app, paste and scan should share one validation path, and a paste should be accepted only as `purepoint-pair:` followed by the QR JSON, at most 4 KB.

## Interfaces

| Interface | Shape |
|---|---|
| Release asset | `purepoint-pointguard-<version>-<x86_64\|aarch64>-linux-gnu.tar.gz` + `.sha256` + GitHub build-provenance attestation |
| Install | `install.sh`: `PU_WITH_PHONE=1` (download) or `PU_PHONE_ARCHIVE=<tarball>` (local); exit 3 for an unsupported libc with `pu`/`pu-engine` still installed |
| Payload | `~/.pu/pointguard/<version>/{bin/node, bin/point-guard-lock, bridge/, node_modules/, runtime-manifest.json}`; manifest `{"schemaVersion":1,"layout":"linux","puVersion":…}` |
| Release version | `pu` compiled with `PU_RELEASE_VERSION=<tag>` in release builds (crate version otherwise); `pu phone` and the manifest must match exactly |
| Unit | `~/.config/systemd/user/pu-pointguard.service`: `ExecStart=<payload>/bin/node <payload>/bridge/main.js --managed`, explicit `Environment=`, `UnsetEnvironment=`, `UMask=0077`, `NoNewPrivileges`, `LockPersonality`, `RestrictRealtime`, `RestrictSUIDSGID`, `SystemCallArchitectures=native`, `Restart=on-failure`, `KillMode=mixed` |
| State | `${XDG_STATE_HOME:-~/.local/state}/purepoint/point-guard/` (0700): `admin-token`, `desktop-chat-token`, `admin.json` (`adminSocket`, never `adminURL`), `trust/`, `admin/admin.sock` (0600 in a 0700 dir) |
| Owner admin | HTTP/1.1 `POST /admin/v1` over the Unix socket, `Authorization: Bearer <admin-token>`; operations as on the Mac plus `devices.rename {deviceId, name}`; `pairing.status` adds `deviceId` once a consumed enrollment's device is durable |
| Phone verify | `GET /pair/verify` (authenticated) adds optional `hostKind`, `hostLabel` |
| Status | `status` adds `hostKind`, `hostLabel` and `remote: {state: listening\|unavailable\|starting, mode, address?, code?, message?, recovery?, commands?}` |
| Paste code | `purepoint-pair:` + the QR JSON |

## Security requirements (parity and Linux-specific)

These apply to the Linux host, and those marked (shared) also apply to the Mac bridge.

- Given a remote listener start, should bind only one explicit tailnet or loopback address, never a wildcard or a public address. (shared)
- Given a remote connection, should accept it only after the TLS handshake with the pinned host identity, and should cap concurrent connections per listener and per device, with absolute pre-auth time limits on enrollment, verify and upgrade requests. (shared)
- Given a QR enrollment, should be single-use, held only in memory, expire within 300 s (default 120 s), and contain no long-lived secret. (shared, unchanged)
- Given device credentials, should store only hashes on the host and compare secrets in a way that does not leak timing. (shared, unchanged)
- Given any WebSocket message or broadcast, should re-authorize the device and check the chat, terminals or workspace grant that applies before acting or sending. (shared, LPH-013)
- Given revocation, should report success only after the revocation is durably written, and should close that device's live sockets immediately. (shared)
- Given the owner admin channel on Linux, should listen only on a Unix socket in a freshly created 0700 directory, set the socket's mode explicitly after bind independent of umask, and still require the admin bearer token. (LPH-006)
- Given the state, trust and admin directories, should create them 0700 and their files 0600, and refuse to start (with recovery text) when an existing entry is a symlink, owned by another uid, or group/other-accessible. (LPH-004)
- Given the systemd unit, should launch with an explicit environment that excludes `NODE_OPTIONS`, `NODE_PATH`, `PI_MOBILE_*`, `PU_*` and owner bootstrap variables from the Pi child, as `runtime.js:214-232` does on the Mac.
- Given an installed bridge payload, should verify its checksum and release signature before activation, refuse to run a payload whose manifest version differs from `pu`, and never overwrite the files of a running version in place. (LPH-002)
- Given logs, should never contain credentials, enrollment tokens, prompts or terminal content. (shared, unchanged)

## Edge Cases

- **SSH session drops during `pu phone pair`**: SIGHUP revokes the enrollment. If the revoke is uncertain, the token still expires within its TTL.
- **Host reboots before `tailscaled` is up**: the unit starts, local admin works, and the remote listener binds when the tailnet appears (LPH-008).
- **Tailscale IP changes** (re-auth or node key reset): the phone's saved endpoint stops matching. Tailscale IPs are normally stable, so the first slice asks the user to re-pair. Endpoint update over the pinned channel is a follow-up.
- **Two bridges on one host** (`pu phone run` while the unit is active): the state lock refuses the second (`lock_busy`) and nothing is adopted.
- **`pu` upgraded but bridge payload not**: `pu phone` reports the version mismatch and does not administer it until `pu phone install`.
- **musl/Alpine host**: install refuses with a clear "glibc ≥ 2.28 required for phone access" message, and `pu`/`pu-engine` keep working.
- **Userspace Tailscale without operator rights**: the bridge runs, `status` shows the exact `tailscale serve` command still to run, and pairing refuses until the serve forward answers.
- **A remote machine of the Linux host goes offline**: the phone shows it as unavailable, and the host and its other machines are unaffected.
- **No lingering**: `pu phone install` warns, as `pu daemon install` does, that the bridge stops at logout.
- **Same phone paired to a Mac and a Linux host**: two independent trust records keyed by hostId, with independent revocation.

## Implementation plan

Implemented on `pu/linux-phone-host`. It builds on PR #264 (chat grants) and PR #265 (owner-only daemon socket, explicit `pu-engine` lookup: the bridge runs the `pu` beside the installed `pu-engine`). The user guide is [`guide/linux-server.md`](../guide/linux-server.md#5-pair-a-phone-or-ipad-optional).

## Risks

- **Size and supply chain**: a Linux bridge payload ships the bridge's ~175 lockfile production packages (some platform-optional) and a Node runtime to servers. Its integrity guarantees must not be weaker than the `pu` binaries'.
- **glibc-only** support excludes Alpine hosts until Node.js publishes official musl builds (LPH-001).
- **Userspace-networking Tailscale** relies on the user being a Tailscale operator, or running one printed `sudo` command, before `tailscale serve` can be applied (LPH-008).
- **Identity minting changes on the Mac too** (LPH-010). New identities are ECDSA P-256 from Node's crypto; existing RSA identities load unchanged. The Mac app still needs an owner Xcode build to confirm.
- **Headless OAuth** coverage depends on which Pi providers offer manual-code or device-code flows. API keys always work.
