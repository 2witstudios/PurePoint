# PurePoint Mobile

A standalone native iPhone chat for vanilla Pi running on your Mac. The bridge owns one persistent RPC process; closing, backgrounding, or disconnecting the phone does not stop Pi. Pi can use Jono’s existing Point Guard skills and `pu` CLI across projects. The app has no PurePoint API dependency.

Everything is scoped to this directory. There are no daemon edits, global installs, desktop app dependencies, build-copy scripts, or cloud services.

## Mac setup

Requirements: Node **22.19+**, existing Tailscale connectivity, existing native Pi provider setup, and installed Point Guard / pu skills. Keep the Mac awake and this terminal process running for remote use.

```sh
cd apps/purepoint-mobile
npm ci --ignore-scripts
```

The lockfile pins **vanilla `@earendil-works/pi-coding-agent` 1.1.0** and patched `ws` 8.22.0 locally. This is neither the customized PageSpace harness nor the installed PageSpace task CLI. Never globally install/link the harness.

First open vanilla Pi locally from the folder you intend it to work in. Use an absolute local executable path because the working folder is deliberate:

```sh
cd /absolute/working/folder
/absolute/path/to/apps/purepoint-mobile/node_modules/.bin/pi
```

Configure/login to your native provider and select your model there. Existing `~/.pi/agent` configuration/auth, extensions, native skills/tools/providers and trusted project resources are preserved. The bridge does not inspect auth files or change provider settings. Verify `/skill:pu` and `/skill:pu-cli` where installed, and check your desired Point Guard skills using `/skill:name`. Existing `~/.agents/skills` symlinks are supported by vanilla Pi. Claude slash commands are not automatically registered as Pi commands.

The bridge automatically generates a cryptographically random pairing secret on first start and stores it privately at `~/.config/pi-mobile/pairing-secret` (mode 600). Subsequent starts reuse it, so your phone stays paired. It never prints the secret. Optional `PI_MOBILE_TOKEN_FILE` selects a different private file; an existing invalid file fails visibly rather than being replaced. The QR transfers the secret to your phone’s Keychain without typing or copying it. Native provider credentials remain separate and unchanged.

Find the Mac’s IPv4 Tailscale address with your installed Tailscale app or `tailscale ip -4`. Then run:

```sh
cd /absolute/path/to/apps/purepoint-mobile
PI_MOBILE_HOST=100.x.y.z \
PI_MOBILE_CWD=/absolute/working/folder \
npm start
```

Use the printed address, such as `ws://100.x.y.z:8787/v1`, in the app’s Connection screen. A literal `100.x.y.z` is an example; replace it with the actual IP. Public, LAN and wildcard binds are refused. Tailnet traffic is encrypted by Tailscale; no port forwarding/public exposure is assumed. Optional `PI_MOBILE_TLS_CERT` and `PI_MOBILE_TLS_KEY` enable `wss` with an owner-provided certificate trusted by iOS. Certificate verification is never bypassed. A `.ts.net` endpoint is also accepted by the app; the listener still binds to an explicit Tailscale IP.

**Desktop and phone connect simultaneously to one live Pi session.** Each authenticated client receives the same revisioned messages, tool activity, queue, dialogs and session changes. Both can send or Stop; the bridge serializes mutations, rejects stale session/run targets, and accepts the first dialog answer. Canceled queued text is recovered only on the device that submitted it. Drafts and delivery receipts remain local. Disconnecting one client leaves other clients and Pi running. Up to 32 clients are allowed; heartbeats and per-client backlog limits isolate disconnected or slow devices. The bridge’s Ctrl-C closes Pi gracefully. This is a foreground Mac process, without launchd or wake guarantees.

The shared protocol is **v1**, at `/v1`. Phone and desktop implement the same initial contract; there are no compatibility endpoints, wire fallbacks or migration layers.

Optional environment settings:

| Setting                                   | Purpose                                                           |
| ----------------------------------------- | ----------------------------------------------------------------- |
| `PI_MOBILE_PORT`                          | Listener port; default 8787                                       |
| `PI_MOBILE_SESSION`                       | Native Pi session path/ID to resume at bridge startup             |
| `PI_MOBILE_PU_SKILL`                      | Explicit installed pu `SKILL.md` if auto-discovery cannot find it |
| `PI_MOBILE_TLS_CERT`, `PI_MOBILE_TLS_KEY` | Owner-managed TLS files for wss                                   |

The launcher discovers installed pu/pu-cli skill files from user skill roots and the PurePoint plugin cache, rather than pinning a versioned cache path. It explicitly adds those skills and checks `get_commands` for successful native skill loading. It clears inherited **PU_AGENT_ID and PU_PROJECT_ROOT**, so the global assistant does not impersonate this builder. Point Guard skills otherwise come from native discovery.

For every cross-project operation, Pi should use **explicit routing**, for example `PU_PROJECT_ROOT=/absolute/project pu status --json`, or run `pu` from that project’s absolute root. The installed CLI resolves this environment variable before cwd and has **no general `--project-root` flag**. The appended [Point Guard context](docs/point-guard.md) explains this without restricting native tools/providers. Check `pu <command> --help` for current command flags.

## QR pairing

On startup, the bridge creates a private offline pairing page at `~/.config/pi-mobile/pairing.html`, using the automatically generated or existing secret file. Open it locally:

```sh
open ~/.config/pi-mobile/pairing.html
```

In the iPhone app, open **Connection → Scan Mac QR code**, allow camera access, and point at the QR. The app saves the secret in Keychain and connects automatically. Both devices still need Tailscale. Manual entry remains available on devices without scanning support and in the simulator.

The QR contains a remote-control credential. The page is mode 600, has no scripts or remote resources, and neither its payload nor its QR is printed to terminal logs. Close the page after pairing; delete it when no longer needed. It remains valid while the underlying secret remains valid. The secret is generated once and reused; restarting does not rotate it. QR pairing supports a secret of 32–1024 bytes without embedded newlines; longer secrets still allow manual pairing.

To recreate the page without restarting a running bridge, run `npm run pair` with the same `PI_MOBILE_HOST`, `PI_MOBILE_PORT`, `PI_MOBILE_TOKEN_FILE` and optional TLS variables as the bridge. This command does not start a listener or Pi process.

## iPhone / Xcode — owner-run

Open **[PurePoint.xcodeproj](PurePoint.xcodeproj)** in Xcode 16 or later. Select the **PurePoint** scheme. The project has a standalone iPhone app target (iOS 17+) and a PurePointTests XCTest target; no script phases or references to the PurePoint app, Cargo or installed binaries.

1. In the PurePoint target’s Signing & Capabilities, choose your team and a unique bundle identifier. Choose the matching team for the test target if needed.
2. Select an iPhone simulator and press **Cmd+R** to build/run. Use **Cmd+U** for the domain tests. For a local fixture, connect to `ws://127.0.0.1:8787/v1`.
3. For a physical iPhone, select your device, keep it on your existing tailnet, accept any local-network permission prompt, and enter the Mac’s Tailscale endpoint and your secret. Build/run with **Cmd+R**.
4. Exercise the owner checks below before relying on remote control.

For everyday physical-device testing, select **PurePoint** (the default) or **PurePoint-Device** and press **Cmd+R**. It builds Release and launches without LLDB; use **PurePoint-Debug** when you need breakpoints. Cmd+U still uses the Debug test configuration. Both schemes install the same app with the same signing and saved pairing/conversation data. No separate backend or bridge restart is needed.

If Xcode displays a blank screen with “libobjc.A.dylib is being read from process memory” or “Looking up debug dylib relative path”, compare the Device scheme or a Home Screen launch. Apple documents [debugger-related startup delays](https://developer.apple.com/forums/thread/800067) and recommends disabling Debug executable to isolate them. The shared-cache warning alone does not prove the app is stalled. Launch logs under subsystem `PurePoint`, category `Launch`, emit `SwiftUI App initialized` and `Chat view appeared`, without credentials or transcript content. A delay before the first marker occurs before this app's App initializer; a gap between markers points to app/model/view initialization. On-device results still require owner verification. If the no-debugger build also pauses, profile that launch with Instruments; don't clear pairing or native sessions.


The plist permits cleartext transport because IP-address `ws` inside Tailscale needs it; the app itself validates endpoints to tailnet/loopback hosts. Use owner TLS/wss if desired. Normal conversation screens contain no RPC controls.

**No Xcode app build, XCTest run in Xcode, signing, simulator launch, screenshots, or physical-device run was performed by this agent.** Those remain owner-controlled. Safe standalone checks below do not establish iOS SDK compilation, actual keyboard behavior, visual rendering or real-device networking.

## Mobile conversation controls

The compact left sidebar contains title-only conversation history, inline search, a header new-chat action and connection settings in its PurePoint footer. Tap the sidebar button or swipe right from the left edge. Selecting an idle conversation resumes it; during a run you can browse read-only and explicitly Stop before resuming. Tool activity appears inline, with grouped expandable calls and outputs. Tap the conversation area to dismiss the keyboard for reading; interactive scroll dismissal also remains available. Connection/loading status occupies the single subtitle beneath PurePoint in the header, so reconnecting does not move the composer. Tap that header to open connection settings.

The composer’s **+** menu offers **Photos** and **Files**. Images are prepared as JPEG and sent through native Pi RPC to an image-capable model. Text files and PDFs with selectable text are sent as named text; PDF binaries/layout and scanned PDF pages are not uploaded. Attachment chips show `Image` or `Text`, and can be removed before sending. Up to four files fit a 512 KiB prepared-data budget; each text attachment is at most 32 KiB and all prompt text at most 64 KiB. PDFs are limited to 30 pages. Attachments wait locally if Pi is busy; ordinary text can still use Send options → Steer / After reply.

Attachment drafts and uncertain submissions are saved privately on the phone, without automatic replay. Photos/files added after submission remain in the composer. The Mac bridge must run this updated version before sending images; the phone checks its capabilities. Restart the bridge only after Pi is idle, with `PI_MOBILE_SESSION` set to the current native session ID/path if you want to resume the same conversation.

Owner check: rebuild in Xcode, inspect composer/keyboard transitions and sidebar gestures in both appearances, select a photo/text/PDF, remove a chip, send to Haiku, and test background/reconnect during acceptance. Native image history currently displays `[Image]` rather than a downloaded thumbnail.

## Deterministic fixture and preview

No provider/API calls or native session writes are made by fixture mode. It uses a real RPC child and the same bridge/controller/network path:

```sh
cd apps/purepoint-mobile
PI_MOBILE_HOST=127.0.0.1 \
PI_MOBILE_TOKEN_FILE=/absolute/private/pairing-secret \
npm run fixture
```

Use your Tailscale IP instead of loopback for an actual phone. The app opens real connection settings; there is no on-device preview option. Xcode’s development preview uses an isolated layout demo. The RPC fixture supports:

| Message              | Exercise                                                                |
| -------------------- | ----------------------------------------------------------------------- |
| Any ordinary message | Streaming prose, code, tool details and accepted receipt                |
| `/fixture-slow`      | Six-second response: type more, Steer / After reply, Stop or disconnect |
| `/fixture-confirm`   | Native confirmation                                                     |
| `/fixture-select`    | Native selection                                                        |
| `/fixture-input`     | Native text input                                                       |
| `/fixture-dialog`    | Native extension editor                                                 |
| `/fixture-draft`     | Extension offers composer text without replacing your draft             |
| `/fixture-error`     | Visible response error                                                  |

The fixture conversation list includes a read-only historical conversation you can resume. Extension dialogs remain pending across connection loss; cancellation sends the correlated response. Pi-native timeouts remove expired dialogs. String widgets, notices and statuses are displayed; custom terminal components and other TUI-only UI are not supported by upstream RPC.

## Verification

Run only the standalone checks from this directory:

```sh
npm test
npm run check
npm audit
swiftc PurePoint/ChatDomain.swift verification/LogicChecks.swift -o /tmp/pi-mobile-logic-checks
/tmp/pi-mobile-logic-checks
swiftc PurePoint/ChatDomain.swift verification/ResponsivenessChecks.swift -o /tmp/pi-mobile-responsiveness-checks
/tmp/pi-mobile-responsiveness-checks
sh verification/check-recovery.sh
swiftc -typecheck PurePoint/ChatDomain.swift PurePoint/PairingSecret.swift PurePoint/ChatModel.swift
swiftc -frontend -parse PurePoint/*.swift PurePointTests/*.swift
plutil -lint PurePoint.xcodeproj/project.pbxproj PurePoint/Info.plist
```

Node tests include byte-fragmented JSONL/Unicode framing, bounded projection, branch/compaction history, child failures, ID correlation, queue ordering and stale Stop races, reconnect partials/dialogs/editor offers, concurrent clients/authentication, a full real fixture WebSocket→RPC flow, read-only native-session browsing with a byte-for-byte unchanged source file, skill discovery, and the **actual published vanilla runtime** in an isolated temporary config with no owner credentials/model calls. Native skill loading is checked against the pinned runtime. TypeScript checks the JS bridge. Standalone Swift logic runs and model/Keychain typecheck use the Mac SDK.

GitHub Actions runs bridge tests and typechecking, standalone Swift logic/responsiveness/recovery checks, and iOS simulator XCTest for mobile changes. The required `Build & Test` check reports on every PR and requires all applicable component jobs to pass; unrelated changes skip component jobs without leaving the required status pending. macOS/Rust changes retain the desktop build and test job. Physical-device and visual acceptance remain owner checks.

Owner acceptance checks:

- In light/dark and large Dynamic Type, read/select prose, expand tools and copy code; inspect VoiceOver control labels, landscape and small-screen spacing.
- Type a multiline/IME draft. Send only with the button; keep typing during acceptance/streaming. Confirm later typing stays intact.
- Queue Steer and After reply during `/fixture-slow`. Stop; restore the canceled text. Confirm a stale Stop cannot affect later work.
- Keep desktop and phone connected: send from either, confirm matching messages/tools, race dialog answers/session changes, queue from both and Stop from the other device. Each receives only its own canceled draft recovery.
- Background/force-close/disconnect the phone while work runs. Reconnect: no resend, one authoritative message per turn, partial output/tools visible. Unacknowledged submissions remain recoverable with an uncertain-delivery explanation.
- Browse history during work. Resuming/new requires an explicit Stop decision while busy. Verify real native tree/compaction sessions display only the active branch and summary markers.
- Exercise all fixture dialogs, dismiss one, reconnect during one, and test an extension editor offer with an existing draft.
- Validate real native provider auth and Point Guard skill use in your selected working folder; direct an explicit cross-project `pu status` operation. Mobile Stop must leave delegated workers running.

## Recovery and bounded limits

Accepted/queued/handled is separate from run completion; `agent_settled` is the idle boundary. Stop serializes `clear_queue` before `abort`, returns canceled text (also retained in bounded reconnect recovery snapshots), targets the observed session epoch/run and does not kill Pi or PurePoint workers. Sessions are listed using native SessionManager metadata, selected using native RPC, and browsed read-only through native in-memory branch semantics. There is no transcript database or Pi JSONL rewriting.

The phone stores drafts and submission recovery receipts only. It never automatically resends an uncertain command. Reconnect sends only `sync`, and replaces the visible projection from the bridge. Receipt IDs are deduplicated within a bounded 2,000-request bridge-lifetime window; this is not durable exactly-once execution. Restarting the bridge cannot recover in-flight receipts or unfinished generation; persisted native Pi conversations can be resumed. The bridge starts a fresh session unless you specify `PI_MOBILE_SESSION` or resume in the app.

RPC records are limited to 8 MiB; read-only history files to 16 MiB; requests to 1 MiB; network backlog to 4 MiB. History shows at most 500 rows with a 1 MiB aggregate wire budget, tool detail has a 256 KiB budget, queue detail 128 KiB, and long text is visibly truncated. Full native history remains in Pi. There are at most 32 pending dialogs and 100 visible tools. Thinking is not exposed; images have text placeholders. Summaries/context edits retain native semantics: the display shows raw selected branch history and summary markers, while Pi independently builds compacted/edited model context.

See [the researched contract and requirements](docs/contract.md) and [validation evidence](docs/validation.md).
