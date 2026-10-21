# iPad App

**Maturity: SPECIFIED** | ID Prefix: IPAD | Dependencies: `product/desktop-app.md`, `product/machines.md`, `product/point-guard-setup.md`, `architecture/desktop-app-integration.md`, `architecture/ipc-api.md`, `research/ipad-ssh-transport.md`

## Purpose

The iPad app is the macOS app minus the local machine. It is the desktop app as if the desktop app could only reach remote machines over SSH. It has literal parity with the Mac app, UI shell included:

- sidebar outline, pane grid and tabs;
- interactive terminals, editor, file tree, diffs and review;
- command palette, hotkeys and the menu bar;
- Schedule, Agents hub, Triggers, Channels and Settings;
- Point Guard.

Every project lives on an SSH-reachable machine running pu-engine, exactly like a remote machine in the Mac app today (`machines.md`). Point Guard chat pairs with a Mac's Point Guard bridge, the same way the iPhone app does. When this ships, an iPad with a hardware keyboard is a complete PurePoint workstation against any machine running pu-engine.

This is not a variant of the iPhone app (`apps/purepoint-mobile`). That app stays a Point Guard chat client with a drawer-based agent viewer, and is unchanged apart from donating `PairingSecret.swift`.

## Conceptual Model

```
iPad app (target purepoint-ipad, same source groups as purepoint-macos)
  Device identity     Ed25519 key in the Keychain → OpenSSH public key to copy
  Host-key pins       one pinned host key per machine (TOFU, mismatch refuses)
  Machine registry    machines.json schema in the app container (not synced)
  Machine (SSH)       one primary SSH connection per machine (+ overflow connections, IPAD-005)
    └─ exec channel per daemon connection: `pu daemon proxy` ⇄ remote ~/.pu/daemon.sock
  DaemonTransport     SSHExecTransport (iPad) | UnixSocketTransport (Mac)
  Projects            always machine-backed: (machineId, projectRoot), DaemonProjectHost
  Point Guard         paired client of a Mac bridge: pinned WSS + device credential
```

- **Machine** has the same meaning as in `machines.md`: a user-chosen name plus an SSH destination. On iPad the destination is always `user@host[:port]`, because iOS has no `~/.ssh/config`, ssh agent or system `ssh`.
- **Daemon connection** means one newline-JSON stream to the machine's daemon. On the Mac it is a Unix socket, either local or `ssh -L` forwarded. On iPad it is one SSH exec channel running `pu daemon proxy`, which copies bytes between its stdio and `~/.pu/daemon.sock` on the machine. The daemon protocol is unchanged (MACH-002).
- **Seam, not fork.** The Mac app already models remote machines end to end: `ProjectKey.machineId`, `DaemonEndpoint.machine(id)`, `DaemonProjectHost`, `MachineState`, `RemoteLayoutSaver`, and the `usesRemoteHost` flags in the terminal and preview. The iPad keeps all of that and swaps only what cannot exist on iOS: the daemon transport, the AppKit view classes, and local-machine services.

## Research Notes

**Researched: 2026-10-20.** The full spike is in `docs/research/ipad-ssh-transport.md` (IPAD-1.1). Key findings:

- Citadel 0.12.1 (`SSHClient.withExec`) streams exec stdin and stdout incrementally and is 8-bit clean. It also runs concurrent exec channels on one `SSHClient`. No raw NIOSSH fallback is needed.
- SwiftNIO SSH, both the fork Citadel pins and upstream, and Citadel itself cannot open `direct-streamlocal@openssh.com` channels. `SSHChannelType` is a closed enum of `session`, `direct-tcpip` and `forwarded-tcpip`. A Unix-socket forward like the Mac's `ssh -L` is therefore not available, and an exec relay is required.
- OpenSSH refuses the 11th concurrent session channel on one connection (`MaxSessions`, default 10). One SSH connection per machine therefore cannot carry every daemon connection of a busy workspace.
- `withExec` masks the remote exit status: its trailing close throws `ChannelError.alreadyClosed`. Exit codes have to be captured inside `perform`.
- `withExec` has no stdin half-close: closing the channel is end-of-stream.
- The exec string runs through the remote login shell with a minimal `PATH`.

Mac-side facts this spec builds on:

- Remote machines (APP-016, `machines.md`) already have full project parity through `DaemonProjectHost`, for files and git over daemon RPC (protocol ≥ 9).
- The Point Guard bridge already authorizes remote devices with a pinned TLS certificate plus a per-device credential (PGSET-004). Each device has a terminals grant of `off`, `view` or `control` (`apps/purepoint-mobile/bridge/admin.js`).
- The iPhone's `PinnedHostSession` and `PairingSecret` (`apps/purepoint-mobile/PurePoint/PairingSecret.swift`) already implement the client side of that trust.

Alternatives considered:

- **A dedicated iPad agent or mTLS protocol** (as in the LAN mesh plan). Rejected for the same reason as in `machines.md`: SSH already supplies identity, encryption and reachability.
- **Syncing the Mac's machine registry.** Rejected (DEC-8). The iPad cannot use the Mac's ssh config aliases or keys, so a synced entry would not connect anyway.

## Decisions

! [IPAD-001] Scope is the macOS app minus the local machine — the owner chose full parity including the UI shell (2026-10-20); with no local device every project is machine-backed, which the Mac's remote-machine path already supports end to end.

! [IPAD-002] Shared source, second target — `purepoint-ipad` is a second target in `apps/purepoint-macos/purepoint-macos.xcodeproj` compiling the same synchronized source groups, with file-level `#if os(macOS)` / `#if os(iOS)` guards and `<Name>+iOS.swift` UIKit twins that keep the Mac type names (DEC-1). Parity then holds by construction, SwiftUI call sites do not change, and synchronized groups avoid per-file pbxproj churn.

! [IPAD-003] AppKit shims for value types, twins for views — on iOS one shim file aliases `NSColor`, `NSFont` and `NSImage` and provides `Color(nsColor:)` (DEC-5). This replaces rewriting about 42 call sites. Only AppKit view classes get real UIKit twins.

! [IPAD-004] Exec-relay transport — daemon traffic goes through a `DaemonTransport` protocol (`openStream() → DaemonByteStream`). `UnixSocketTransport` keeps the Mac unchanged. `SSHExecTransport` opens one Citadel exec channel running `pu daemon proxy` per daemon connection (DEC-2, DEC-3) — `direct-streamlocal` is unavailable in SwiftNIO SSH (research note), and an exec relay works through any sshd that permits commands.

! [IPAD-005] One primary SSH connection per machine, overflow on refusal. Every daemon channel opens on the machine's primary SSH connection. When the server refuses a session channel (`channelSetupRejected`, which is OpenSSH `MaxSessions`), the transport opens one additional SSH connection to the same machine, with the same identity and pinned host key, and retries there. Further refusals add further connections. An overflow connection closes once it carries no channels. Rationale: one connection is the plan's design, and it is the cheapest path. The 10-channel default cap is a server limit the app cannot change, and the Mac's attach, grid, status and fs streams can exceed it.

! [IPAD-006] Relay never starts a daemon — `pu daemon proxy` exits non-zero when no daemon answers (MACH-003); the iPad shows the machine as daemon-stopped and offers an explicit Start daemon action that runs `pu health` over SSH, the Mac's start command.

! [IPAD-007] Owner authority — an SSH session runs as the owner's account, so the iPad has the same full daemon authority as a Mac remote machine (DEC-4); the MESH grant model does not apply because this is the owner's own client, not a peer daemon.

! [IPAD-008] Device SSH key in the Keychain. Each install generates an Ed25519 identity. The private key is stored as a generic-password Keychain item with `kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly`, and is never synchronizable or exported. The app shows the OpenSSH public key for copying, and the user can import an OpenSSH Ed25519 private key that replaces the generated one. Rationale: iOS has no ssh agent or `~/.ssh`, so unlike MACH-001 the app must hold a key. Keeping it device-only means a backup or iCloud Keychain sync never carries it off the device.

! [IPAD-009] TOFU host-key pinning per machine — the first connection shows the host key's type and SHA256 fingerprint and requires explicit confirmation before authentication proceeds; the confirmed key is pinned for that machine. A different key refuses the connection and never auto-accepts; only a deliberate Reset Host Key action clears the pin, after which the next connection asks again. Rationale: there is no `known_hosts` on iOS, and silent acceptance would let a network attacker take the owner's full daemon authority.

! [IPAD-010] Container registry, not synced — the iPad keeps its own registry with the `machines.json` schema in its app container, without flock or `~/.ssh/config` (DEC-8). The schema stays one format across platforms, and it is not synced, because a Mac entry's ssh alias and key would not work from the iPad.

! [IPAD-011] Point Guard is a paired client. On iPad, `PiChatModel` connects to a paired Mac's bridge over pinned WSS with a per-device credential. It reuses the iPhone's `PinnedHostSession` and `PairingSecret.swift`, shared as a file reference the way `apps/shared/ChatMarkdown` is. Pairing is by QR. The bridge treats the iPad as a paired device under the existing Off/View/Control terminals grants, with no bridge change. The iPad never hosts Point Guard. Rationale: the owner decided this (2026-10-20). The iPad has no machine of its own to host Pi on, and PGSET-004 already defines device trust.

! [IPAD-012] Machine-scoped daemon views. Agents Hub, Schedule, Triggers and agent settings target the daemon of the machine that owns the current sidebar selection. Each names that machine in its header, and offers a machine picker when nothing is selected. Rationale: on the Mac these views are scoped to the local daemon (APP-016). The iPad has no local daemon, and the selected machine is the iPad's equivalent of "this computer".

! [IPAD-013] Point Guard has no local workspace. On the Mac, the Point Guard item opens a folder-root workspace whose terminals, agents and files run on the local daemon (APP-004). On iPad the Point Guard item opens the chat alone. Agent, terminal and file tabs are not offered in that workspace, because it has no machine. The chat surface itself (transcript, composer, sessions, run controls) is the same shared view. Rationale: there is no machine to own those processes. Spawning on a paired Mac would need a registered machine, and the owner can open that machine's projects directly.

! [IPAD-014] Local notifications only — agent-signal notifications and the app badge are posted with `UNUserNotificationCenter` while the app is running or suspended (DEC-10); delivery after termination needs APNs and stays in the iOS notifications plan.

! [IPAD-015] Legacy chat guarded — `ChatState`, `SessionListState`, `StreamAccumulator`, `ChatAreaView`, `ChatInputView` and `ConversationSidebarView` are unused in the Mac app and are guarded `#if os(macOS)`, not ported (DEC-9); removing them is a separate Mac cleanup.

! [IPAD-016] iPadOS 26 minimum, multiple scenes — matches the macOS 26.1 baseline for the menu bar, windowing and SwiftUI parity (DEC-7); each window gets its own shell like the Mac `WindowGroup`.

! [IPAD-017] Builders build only the iPad scheme — `xcodebuild -scheme purepoint-ipad -destination 'generic/platform=iOS Simulator'` build and test are allowed for agents because the target has no cargo or install phase (DEC-6); the macOS target is built only by the owner.

## Requirements

### Scope and shell

- REQ-IPAD-001: Given the iPad app, should present the Mac app's shell: sidebar outline, pane grid with tab stacks, workspace files sidebar, command palette, Schedule, Agents hub, Triggers, Channels and Settings, all driven by the same state types as the Mac.
- REQ-IPAD-002: Given the iPad app, should offer no local machine. Every project is machine-backed on `DaemonProjectHost`, and the sidebar shows only the MACHINES section.
- REQ-IPAD-003: Given a hardware keyboard, should deliver the Mac key bindings from `KeyBindingState` as `UIKeyCommand`s posting the same `.hotkeyAction` notifications. The app's `.commands` menus should appear in the iPadOS menu bar.
- REQ-IPAD-004: Given an agent pane, should run an interactive terminal fed by `DaemonAttachSession`, with Ctrl, Alt, Esc and arrow keys, resize claims, scrollback scrolling, selection copied to `UIPasteboard`, and links opened with `UIApplication.open`. It should keep the Mac's remote-terminal rules from `machines.md`.
- REQ-IPAD-005: Given a file pane, should browse, edit and save with syntax highlighting, a line-number gutter and Markdown preview. Saves keep the remote conflict and size rules from `machines.md`.
- REQ-IPAD-006: Given review changes, should render the same diffs and inline code as the Mac (APP-010, APP-015).
- REQ-IPAD-007: Given multiple iPad windows, should give each window its own shell, as each Mac window has.
- REQ-IPAD-008: Given a Mac action that opens something locally (Reveal in Finder, Open Local Copy, open in default app), should present a share sheet or Quick Look of the fetched local copy. "Reveal in Finder" itself is not offered.
- REQ-IPAD-009: Given Open Project, should pick a machine and browse its folders through daemon `fs_list` in folder-browsing mode (`directories_only`, `machines.md`), then open the chosen folder as a project root on that machine with the daemon's `init`, the request the Mac sends when it opens a local project. The picker follows the Mac open panel: back/forward, a folder pop-up and path bar, a show-hidden toggle, Cancel/Open, and the panel's keys. It offers no New Folder, because the daemon creates no directories on request. `/`, the home folder and its ancestors cannot be opened, matching the daemon's rule.
- REQ-IPAD-010: Given Agents Hub, Schedule, Triggers or agent settings, should act on the daemon of the machine that owns the current selection and name that machine (IPAD-012).
- REQ-IPAD-011: Given the app moving to the background, should pause polling and save the workspace layouts and selection a Mac quit saves, since iOS may end a suspended app without notice. Given it returning to the foreground, should refresh state and resume streams. Stream reconnect loops wait for the foreground rather than spend their retries while suspended.

### Exec-relay transport

- REQ-IPAD-012: Given a daemon connection to a machine, should open one SSH exec channel running the relay command (Interfaces) on that machine's SSH connection, and frame newline JSON over it exactly as over a Unix socket.
- REQ-IPAD-013: Given concurrent daemon connections to one machine, should multiplex them as channels on the machine's primary SSH connection.
- REQ-IPAD-014: Given a channel open refused by the server's session limit, should carry that connection on an overflow SSH connection to the same machine (IPAD-005). It should close the overflow connection when it carries no channels.
- REQ-IPAD-015: Given a dropped SSH connection, should fail the daemon streams on it with the same errors a closed Unix socket produces. It should then reconnect with backoff, and streams should resubscribe as they do after a daemon restart on the Mac.
- REQ-IPAD-016: Given a relay that exits with the "no daemon" status, should show the machine as connected with the daemon stopped, and should offer Start daemon. It should never start a daemon automatically (MACH-003).
- REQ-IPAD-017: Given Start daemon, should run the Mac's start command (`machines.md`) as an SSH exec with a 60 s limit, then probe health again.
- REQ-IPAD-018: Given `pu` missing on the machine (exit 127), should report an actionable error that names the machine and says PurePoint must be installed there.
- REQ-IPAD-019: Given a daemon with a different protocol version, should show the machine as incompatible and make no other requests (as on the Mac).

### SSH identity and host-key pinning

- REQ-IPAD-020: Given first launch, should generate an Ed25519 identity and store its private key in the Keychain as device-only and non-synchronizable.
- REQ-IPAD-021: Given the identity, should show its OpenSSH public key (`ssh-ed25519 <base64> purepoint-ipad`) with a Copy action, in Settings and in Add Machine.
- REQ-IPAD-022: Given an imported OpenSSH Ed25519 private key (unencrypted, or passphrase-protected with the passphrase entered once), should store the decrypted key in the Keychain in place of the generated one, and use it for every machine. The passphrase is never stored.
- REQ-IPAD-023: Given an import of another key type or a malformed key, should refuse it with a message and keep the current identity.
- REQ-IPAD-024: Given a first connection to a machine, should show the host key type and SHA256 fingerprint and continue only after explicit confirmation. It should then pin that key for the machine.
- REQ-IPAD-025: Given a pinned machine that presents a different host key, should refuse before authentication and show both fingerprints. It should keep refusing until the user deliberately chooses Reset Host Key, after which the next connection asks for confirmation again.
- REQ-IPAD-026: Given a machine removed from the registry, should delete its host-key pin.
- REQ-IPAD-027: Given any log, error message, page or crash report, should never include private key material or a passphrase.

### Machine registry

- REQ-IPAD-028: Given the iPad registry, should read and write the `machines.json` schema (`machines.md`) at `Application Support/PurePoint/machines.json` in the app container. Writes are atomic (temp file plus rename), with no flock and no `~/.ssh/config`.
- REQ-IPAD-029: Given Add Machine, should collect name, host, port (default 22) and user. It should store them as `ssh_host` `user@host` or `user@host:port`, with an IPv6 literal in brackets. Names follow the Mac's validation rules.
- REQ-IPAD-030: Given Add Machine's first connect, should run the host-key confirmation, then a health probe. It should save the machine only after the probe succeeds, then list the machine's projects in the sidebar.
- REQ-IPAD-031: Given a registered machine, should show the Mac machine-row states (Connecting, project count, Daemon stopped, Incompatible, Offline, Disconnected), plus Host key changed while a pin mismatch is refusing.

### Point Guard pairing

- REQ-IPAD-032: Given Point Guard setup on iPad, should pair with a Mac by scanning its QR v1 enrollment code (PGSET-004) through a VisionKit scanner. It should store the trusted host record (endpoint, host id, certificate SHA-256, device id, credential) in the Keychain as device-only.
- REQ-IPAD-033: Given a paired Mac, should connect `PiChatModel` to its WSS endpoint and pin the certificate's SHA-256 before releasing the bearer credential.
- REQ-IPAD-034: Given close code 4001, a pin mismatch or a host-identity mismatch, should stop reconnecting and require deliberate re-pairing.
- REQ-IPAD-035: Given saved Macs, should list them and reconnect to a chosen one.
- REQ-IPAD-036: Given the bridge's device grants, should behave as a paired device under the existing Off/View/Control terminals grant, with no bridge change.
- REQ-IPAD-037: Given the Point Guard composer, should attach photos through `PhotosPicker` and files through `fileImporter`, with the Mac's attachment limits.
- REQ-IPAD-038: Given Point Guard setup on iPad, should offer no hosting, Tailnet setup or phone management.

### Notifications

- REQ-IPAD-039: Given an agent signal while the app is running or suspended, should post the same local notification as the Mac through `UNUserNotificationCenter`. It should set the app badge to the unread count in place of the dock tile.

### Exclusions

- REQ-IPAD-040: Given the exclusion list (Interfaces), should not offer any listed feature on iPad, and should hide its Settings rows and menu items rather than show disabled placeholders.

## Sum Sheet

iPad = Mac − local machine. Projects ⇒ machine-backed. Daemon connection = exec channel running `pu daemon proxy`. Channels share one SSH connection per machine, with overflow connections on refusal. No daemon ⇒ explicit Start. Identity = device-only Keychain Ed25519. Host key = TOFU pin, and a mismatch ⇒ refuse until reset. Registry = container `machines.json`, unsynced. Point Guard = paired client over pinned WSS, never a host. Notifications are local only.

## Interfaces

### Relay command (engine)

`pu daemon proxy` connects to the daemon socket the CLI would use (`~/.pu/daemon.sock`, or `--socket`). It then copies stdin to the socket and the socket to stdout, byte for byte, until either side closes, and exits 0. It performs no handshake and never starts a daemon.

| Exit | Meaning |
|---|---|
| 0 | Either side closed after a successful connect |
| 69 (`EX_UNAVAILABLE`) | No daemon answers on the socket (missing socket or connection refused). Nothing was started. |
| 1 | Any other failure |

Diagnostics go to stderr only. stdout carries daemon bytes only.

The iPad runs it as one SSH exec string, through the same `/bin/sh` wrapper as the Mac's Start daemon command, so it works whatever the remote login shell and `PATH` are:

```
/bin/sh -c 'PATH="$HOME/.pu/bin:$HOME/.cargo/bin:$PATH"; export PATH; exec pu daemon proxy'
```

Exit 127 from this command means `pu` is not installed. Because Citadel's `withExec` masks exit codes on close, `SSHExecTransport` reads the exit status and stderr inside `perform` (research note).

### Transport seam (app)

```swift
protocol DaemonByteStream: AnyObject, Sendable {
    func write(_ data: Data) async throws
    func read() async throws -> Data?   // nil = closed by the other side
    func close()
}
protocol DaemonTransport: Sendable {
    func openStream() async throws -> DaemonByteStream
}
```

- `DaemonClient`, `DaemonAttachSession`, `DaemonGridSubscription` and `DaemonStatusSubscription` open their connections only through `DaemonTransport`.
- `sendBlocking` keeps its signature and timeout.
- `DaemonEndpoint.local` and `.machine(id)` resolve to `UnixSocketTransport` on macOS, at today's paths.
- On iOS, `.machine(id)` resolves to the injected `SSHExecTransport`, and `.local` does not exist as a reachable endpoint.

### Device identity and pins (Keychain)

| Item | Class | Service / account | Accessibility |
|---|---|---|---|
| SSH identity | generic password | `PurePoint.SSHIdentity.v1` / `default` (raw 32-byte Ed25519 private key) | `AfterFirstUnlockThisDeviceOnly`, not synchronizable |
| Host-key pin | generic password | `PurePoint.HostKeyPin.v1` / `<machine id>` (OpenSSH public key line) | `AfterFirstUnlockThisDeviceOnly`, not synchronizable |
| Point Guard host | generic password | `PiMobile.TrustedHost.v1` / `<endpoint>` (`TrustedHost` JSON, from the shared `PairingSecret.swift`) | `AfterFirstUnlockThisDeviceOnly` |

Fingerprints are shown in OpenSSH form: `SHA256:` followed by unpadded base64 of the SHA-256 of the key blob.

### Machine registry (app container)

The schema is the same as `~/.pu/machines.json` (`machines.md`):

```json
{ "version": 1, "machines": [ { "id": "UUID", "name": "studio", "ssh_host": "jono@studio.local:22" } ] }
```

It lives at `<container>/Library/Application Support/PurePoint/machines.json`, with file protection `completeUntilFirstUserAuthentication`. It is not synced with any Mac.

### Debug launch arguments

Debug builds (`#if DEBUG`) take launch arguments that let scripts connect a simulator to a test machine without touching the UI. Each one runs the same code the UI runs. Release builds ignore them.

| Argument | Effect |
|---|---|
| (every launch) | Writes the device's OpenSSH public key line to `<data container>/Documents/purepoint-debug/id_ed25519.pub`. Read it with `xcrun simctl get_app_container <udid> com.2witstudios.PurePoint.ipad data`. |
| `-PUDebugAddMachine user@host[:port][,name]` | Fills Add Machine with that destination and name (default: the host's first label, or `machine` for an IP address), shows the sheet and submits it, as if the user tapped Add. It does nothing when a machine with that destination is already registered. |
| `-PUDebugTrustHostKey YES` | Confirms a first-use host key as if the user tapped Trust and Connect, both in Add Machine and for registered machines that ask. A changed host key is still refused (IPAD-009). |

`apps/purepoint-macos/scripts/ipad-test-sshd.sh start <worktree> [--port N] [--sim <udid>]` runs a throwaway sshd for these:
- It is the system's `/usr/sbin/sshd` binary, run unprivileged on 127.0.0.1 only, with its own host key, `authorized_keys` and config in a temp dir. It never touches `~/.ssh` or the system sshd.
- `pu` resolves to `<worktree>/target/debug/pu`.
- It authorizes the simulator's key from the container file above.
- It prints the port and user.

`install-key` re-reads the key after the app's first launch. `stop` ends the sshd and removes its temp dir.

Typical run:
1. `cargo build -p pu-cli`.
2. Install and launch the app once.
3. `ipad-test-sshd.sh start "$PWD" --sim <udid>`.
4. Relaunch: `xcrun simctl launch --terminate-running-process <udid> com.2witstudios.PurePoint.ipad -PUDebugAddMachine <user>@127.0.0.1:<port> -PUDebugTrustHostKey YES`.

### Exclusion list

These Mac features are excluded on iPad:

- **Local machine.** No local daemon (`DaemonLifecycle`, `--managed` auto-start, quit-time shutdown), and no local projects or `LocalProjectHost`. Also excluded: the `ManifestWatcher`, `WorktreeWatcher` and `FileTreeWatcher` local watchers, and `shutdownWithSuspend`.
- **Installers and updates.** No CLI install (`CLIInstaller`) and no Sparkle updates (`CheckForUpdatesViewModel`, About/updates rows).
- **Point Guard hosting.** No hosting Point Guard (`PointGuardServiceModel`, `PointGuardApplicationDelegate`), no `PointGuardTailnet` setup, and no pairing phones to the iPad or managing devices. The Point Guard local workspace's terminals, agents and files are also excluded (IPAD-013).
- **Local conversation indexes.** No Claude, Codex or OpenCode indexes over `~/.claude`, `~/.codex` or `~/.local/share/opencode`, and no `ClaudeProcess`.
- **Legacy chat (DEC-9).** `ChatState`, `SessionListState`, `StreamAccumulator`, `ChatAreaView`, `ChatInputView` and `ConversationSidebarView` are excluded.
- **Local-only features.** Excluded: the system ssh, ssh config aliases, the ssh agent, and the `ssh -L` forward with its `forward.lock`. Also excluded: `HotkeyMonitor` global hotkeys (replaced by `UIKeyCommand`) and Reveal in Finder (replaced by share sheet or Quick Look).
- **Belongs to other plans.** MESH peer-daemon federation, APNs or background notifications after termination, and syncing the machine registry with a Mac.
- **The iPhone app.** No changes to the iPhone app's UI.

## Edge Cases

- **More than ten daemon streams on one machine.** The server refuses the 11th session channel (`MaxSessions`), so the stream opens on an overflow SSH connection (IPAD-005). A server with `MaxSessions 1` still works, at one SSH connection per stream.
- **Server forbids exec** (`ForceCommand`, a restricted shell). The relay never prints a health line, so the probe times out. The machine shows offline with the stderr text, if there was any.
- **Relay exits immediately with 69.** The machine shows Daemon stopped (IPAD-006). The status is read inside `perform`, because `withExec` reports `alreadyClosed` (research note).
- **Login shell prints a banner or MOTD on exec.** Exec sessions get no PTY, and OpenSSH does not print the MOTD for them. A `.bashrc` that writes to stdout would corrupt the stream, so a health response that does not parse is reported as "unexpected output from the login shell" naming the machine.
- **Host key rotated legitimately.** The connection is refused, as for an attack. The user compares the fingerprints, chooses Reset Host Key, and confirms the new key.
- **Same host added twice under different names.** These are two machines with separate pins and connections. Project identity is `(machineId, root)` (MACH-005), so they are distinct projects.
- **Device restored from backup or migrated.** Device-only Keychain items do not restore. The app generates a new identity, which must be authorized on each machine again, and every machine asks for host-key confirmation again. The container registry may restore, so the machines appear, but connecting them needs the new key.
- **Key import while machines are connected.** Existing SSH connections keep running. New connections use the imported key, and the app reconnects machines that fail authentication with it.
- **App suspended with streams open.** iOS may drop the TCP connection. On foreground the transport reconnects, and the streams resubscribe (REQ-IPAD-011, REQ-IPAD-015). Remote agents keep running (MACH-004).
- **Pi bridge revokes the iPad.** The bridge closes with 4001, and the iPad requires re-pairing (REQ-IPAD-034). Machines and SSH are unaffected.
- **Paired Mac is also a registered machine.** They are independent: Point Guard uses the bridge's WSS, and projects use SSH. Revoking one leaves the other working.
