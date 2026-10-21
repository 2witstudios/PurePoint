# PurePoint Mobile

Native iPhone chat shares the Mac Point Guard's existing Pi session. Desktop and phone see the same conversation, tools and queue. Stop affects the observed Pi run and preserves device-specific draft recovery; the Agents surface shows delegated PurePoint workers and ordinary Shell panes across this Mac and connected machines, separately from Point Guard chat. Each newly paired device starts with Full access; the Mac owner can lower it per device to Terminals only, View only or No access in Point Guard setup. The same grants decide Point Guard chat access (see below). Remote agents require this Mac, its bridge, and its existing machine forwards to remain connected. The phone never uses SSH.

## Setup with the Mac app

Install the reviewed PurePoint Mac app bundle. Open Point Guard → **Setup**, choose a working folder, and configure a supported provider through browser login where offered or an API key. The app starts its packaged Node/Pi bridge and bundled pu CLI/instructions; end users need no repository, external Node, npm, Pi installation, skill files or terminal process. Provider credentials and native conversations stay in private Pi storage outside the app bundle. Apply provider/model/folder changes while Pi is idle.

Open **Connect phone** in the Mac app while both devices use your existing Tailscale network. In the iPhone app, open **Connections** and tap **Scan pairing code** under **Pair a computer**, then scan the visible short-lived QR. It enrolls once, verifies the Mac's TLS certificate pin, and saves a separate per-device credential and host identity in the iPhone Keychain. The QR never contains provider credentials or the saved device bearer. Close/new-code revokes an outstanding enrollment; a failed revocation is reported as uncertain until retry or expiry.

### Pairing with a Linux host

On a headless Linux host, run `pu phone pair` and scan the terminal QR the same way. Over SSH or in a narrow terminal, or with `pu phone pair --text`, it prints a one-line `purepoint-pair:` code instead: copy it, then in the iPhone app open **Connections** and tap **Paste code** under **Pair a computer** (or expand **Enter code manually** and paste it there). The app accepts only text that starts with exactly `purepoint-pair:`, trims surrounding whitespace, rejects input over 4 KiB, and then runs the same validation as a scanned QR, including the certificate pin, expiry and the Tailscale endpoint allow-list.

After the pinned connection verifies, a host may describe itself (`hostKind` `mac` or `linux`, and a short `hostLabel`). The app uses this only for display: the label becomes the default name, Linux hosts get a server icon, and re-pair guidance says to run `pu phone pair` on that host. Hosts stay keyed by their verified host identity, so a Mac and a Linux host with the same label remain separate entries. Older bridges send neither field and behave as before.

Saved devices reconnect on launch, foreground and network recovery with bounded retries. Revocation, a changed host identity or lost trust requires a deliberate new pairing code. The host can revoke an individual device without resetting others; revoked devices are removed from its list and must pair again to return. Legacy shared-token/plaintext phone pairing is unsupported and requires re-pair. Tailscale provides reachability; pinned TLS and device credentials provide authorization. Local Mac setup/chat remain available without Tailscale; reconnect Tailscale and restart the idle service to restore phone reachability.

The service runs for the Mac app lifetime. Restart/update restores selected native session, provider/folder and device trust, but cannot recover unfinished generation or bridge-lifetime receipts. Uncertain prompts are never silently resent. Use the native status, recovery guidance and folder picker when startup fails; the app never adopts or stops an unknown process.

## Development and package proof

The lockfile pins vanilla `@earendil-works/pi-coding-agent` **1.1.0** and `ws` **8.22.0**. Development requires Node22.19+ and `npm ci --ignore-scripts`; this is separate from end-user installation. Do not globally install the harness or overwrite running pu binaries. The managed launcher uses bundled versioned support/instructions and explicit CLI routing; existing native Pi resources remain compatible. Cross-project operations use `PU_PROJECT_ROOT=/absolute/project pu ...` or an explicit cwd, never an inherited builder identity.

`runtime/package-runtime.py` stages the supported Node22.23.3, complete production graph and pu helper into an app artifact. `runtime/sign-runtime.py` signs nested code before the outer app, with Node JIT entitlement. Nonpublishing CI (arm64 on pull requests that touch packaging inputs; x64 and universal on `main`, nightly and manual runs) verifies arm64, x64 and universal resources, signatures and relocated empty-HOME/stripped-PATH launch, then state-preserving bundle replacement. Ad-hoc proof does not establish Developer ID/notarization or physical-device acceptance. The normative [setup/trust contract](../../docs/product/point-guard-setup.md) defines private state and listener/admin boundaries.

## iPhone / Xcode — owner-run

Open **[PurePoint.xcodeproj](PurePoint.xcodeproj)** in Xcode 16 or later. Select the **PurePoint** scheme. The project has a standalone iPhone app target (iOS 17+) and a PurePointTests XCTest target; no script phases or references to the PurePoint app, Cargo or installed binaries.

1. In the PurePoint target’s Signing & Capabilities, choose your team and a unique bundle identifier. Choose the matching team for the test target if needed.
2. Select an iPhone simulator and press **Cmd+R** to build/run. Use **Cmd+U** for the domain tests. Standalone verification scripts provide isolated loopback desktop and pinned-TLS phone fixtures.
3. For a physical iPhone, select your device, keep it on your existing tailnet, accept any local-network permission prompt, then scan a new enrollment QR from the Mac app. Build/run with **Cmd+R**.
4. Exercise the owner checks below before relying on remote control.

For everyday physical-device testing, select **PurePoint** (the default) or **PurePoint-Device** and press **Cmd+R**. It builds Release and launches without LLDB; use **PurePoint-Debug** when you need breakpoints. Cmd+U still uses the Debug test configuration. Both schemes install the same app with the same signing and saved pairing/conversation data. No separate backend or bridge restart is needed.

If Xcode displays a blank screen with “libobjc.A.dylib is being read from process memory” or “Looking up debug dylib relative path”, compare the Device scheme or a Home Screen launch. Apple documents [debugger-related startup delays](https://developer.apple.com/forums/thread/800067) and recommends disabling Debug executable to isolate them. The shared-cache warning alone does not prove the app is stalled. Launch logs under subsystem `PurePoint`, category `Launch`, emit `SwiftUI App initialized` and `Chat view appeared`, without credentials or transcript content. A delay before the first marker occurs before this app's App initializer; a gap between markers points to app/model/view initialization. On-device results still require owner verification. If the no-debugger build also pauses, profile that launch with Instruments; don't clear pairing or native sessions.


Remote phone transport requires pinned WSS. The plist enables `NSAllowsArbitraryLoads` so URLSession can authenticate self-signed hosts at user-selected Tailscale IP addresses using the QR certificate pin. Do not add `NSAllowsLocalNetworking`: on current iOS it overrides that setting and re-enables ATS, which can reject pairing before the pin delegate runs. Endpoint validation still requires WSS and an allowed address, and the session refuses mismatched certificates and redirects. Normal conversation screens contain no RPC controls.

Agents never run local app build phases. Full app/XCTest and temporary ad-hoc artifact proof run through nonpublishing CI; owner Xcode/device acceptance and protected production signing remain separate. Consult the PR exact-head check results for actual proof, rather than inferring acceptance from standalone checks.

## Mobile conversation controls

To name your computers, tap the Point Guard chat header to open **Connections**, then tap the pencil beside a computer under **Your computers**. Enter a name and tap **Save**. Names are saved on this iPhone, shown in the picker and chat header, and retained when you pair the same computer again; a name you choose is never replaced by the host's label. The address beneath each name distinguishes computers with matching names.

The left navigator opens Point Guard, Projects, pinned/recent agents and Connections. With Control, an open agent's terminal takes the phone's size while you view it (its Mac view reflows too) and returns to the previous size when you leave; view-only phones and older daemons keep the agent's size, double-tap to fit width. Tap the terminal or the keyboard button beside the agent prompt to hide the keyboard. Point Guard conversation history opens from its right-side toolbar control. Selecting an idle conversation resumes it; during a run you can browse read-only and explicitly Stop before resuming. Tool activity appears inline, with grouped expandable calls and outputs. Tap the conversation area to dismiss the keyboard for reading; interactive scroll dismissal also remains available. Connection/loading status occupies the single subtitle beneath Point Guard in the header, so reconnecting does not move the composer. Tap that header to open connection settings.

The composer’s **+** menu offers **Photos** and **Files**. Images are prepared as JPEG and sent through native Pi RPC to an image-capable model. Text files and PDFs with selectable text are sent as named text; PDF binaries/layout and scanned PDF pages are not uploaded. Attachment chips show `Image` or `Text`, and can be removed before sending. Up to four files fit a 512 KiB prepared-data budget; each text attachment is at most 32 KiB and all prompt text at most 64 KiB. PDFs are limited to 30 pages. Attachments wait locally if Pi is busy; ordinary text can still use the Steer menu → Steer / After reply.

Attachment drafts and uncertain submissions are saved privately on the phone, without automatic replay. Photos/files added after submission remain in the composer. The Mac bridge must run this updated version before sending images; the phone checks its capabilities. Restart Point Guard through the Mac app only after Pi is idle; the managed service restores the selected native conversation.

Owner check: rebuild in Xcode, inspect composer/keyboard transitions and sidebar gestures in both appearances, select a photo/text/PDF, remove a chip, send to Haiku, and test background/reconnect during acceptance. Native image history currently displays `[Image]` rather than a downloaded thumbnail.

## Deterministic fixture and preview

No provider/API calls or native session writes are made by fixture mode. It uses a real RPC child and the same bridge/controller/network path:

```sh
cd apps/purepoint-mobile
PI_MOBILE_HOST=127.0.0.1 \
PI_MOBILE_TOKEN_FILE=/absolute/private/pairing-secret \
npm run fixture
```

This CLI fixture is loopback-only and cannot authorize a phone. Use `python3 verification/check-trust.py` for an isolated real TLS enrollment/model fixture, or the Mac app QR for owner device checks. The app opens real connection settings; there is no on-device preview option. Xcode’s development preview uses an isolated layout demo. The RPC fixture supports:

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
python3 verification/check-trust.py
swiftc -typecheck PurePoint/ChatDomain.swift PurePoint/PairingSecret.swift PurePoint/ChatModel.swift
swiftc -frontend -parse PurePoint/*.swift PurePointTests/*.swift
plutil -lint PurePoint.xcodeproj/project.pbxproj PurePoint/Info.plist
```

Node tests include byte-fragmented JSONL/Unicode framing, bounded projection, branch/compaction history, child failures, ID correlation, queue ordering and stale Stop races, reconnect partials/dialogs/editor offers, concurrent clients/authentication, a full real fixture WebSocket→RPC flow, read-only native-session browsing with a byte-for-byte unchanged source file, skill discovery, and the **actual published vanilla runtime** in an isolated temporary config with no owner credentials/model calls. Native skill loading is checked against the pinned runtime. TypeScript checks the JS bridge. Standalone Swift logic runs and model/Keychain typecheck use the Mac SDK.

GitHub Actions runs bridge tests and typechecking, standalone Swift logic/responsiveness/recovery checks, and iOS simulator XCTest for mobile changes; see [CI routing](../../docs/contributing/testing.md#ci-routing). The required `Build & Test` check reports on every PR and requires all applicable component jobs to pass; unrelated changes skip component jobs without leaving the required status pending. macOS/Rust changes retain the desktop build and test job. Physical-device and visual acceptance remain owner checks.

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

The phone stores drafts and submission recovery receipts only. It never automatically resends an uncertain command. Reconnect sends only `sync`, and replaces the visible projection from the bridge. Receipt IDs are deduplicated within a bounded 2,000-request bridge-lifetime window; this is not durable exactly-once execution. Restarting the bridge cannot recover in-flight receipts or unfinished generation; persisted native Pi conversations can be resumed. The managed bridge restores its durable selected native session, including a setup-only conversation before its first prompt.

RPC records are limited to 8 MiB; read-only history files to 16 MiB; requests to 1 MiB; network backlog to 4 MiB. History shows at most 500 rows with a 1 MiB aggregate wire budget, tool detail has a 256 KiB budget, queue detail 128 KiB, and long text is visibly truncated. Full native history remains in Pi. There are at most 32 pending dialogs and 100 visible tools. Thinking is not exposed; images have text placeholders. Summaries/context edits retain native semantics: the display shows raw selected branch history and summary markers, while Pi independently builds compacted/edited model context.

See [the researched contract and requirements](docs/contract.md) and [validation evidence](docs/validation.md).


## Workspace management authorization and recovery

Workspace management is part of **Full access**, the default for newly paired devices. Devices paired before that default keep their stored grants (Workspace management off unless granted). Mutations also require terminal Control, so **Terminals only** and lower cannot change workspaces. View can browse registered projects, empty workspaces, worktrees and agents; Off denies the phone's direct fleet requests (`agents`, terminals, workspace operations). Registered machine IDs scope all project paths and agent/worktree IDs. No phone command, cwd, environment, SSH or arbitrary IPC passthrough exists. Point Guard chat follows the same grants, because Point Guard can list, message, spawn and stop agents on request.

## Point Guard chat access

Point Guard has a shell and the `pu` CLI, so chatting with it is effectively full control of this Mac. Chat access therefore follows the existing grants instead of a separate toggle:

- **Agents and terminals: Off**: no chat. The bridge rejects every chat operation and sends the device no conversation snapshots or editor offers. The app locks chat with an explanation.
- **View**, or **Control** without Workspace management: read-only chat. The device can view the live conversation and history (`sync`, `sessions`, `history`) but cannot send, steer, stop, start or resume conversations, or answer extension prompts. The app shows the transcript with the composer replaced by an explanation. Read-only chat still shows whatever Point Guard reports about agents and this Mac.
- **Control and Workspace management**: full chat, so the device can ask Point Guard to run commands and manage agents on this Mac.

There is no separate chat field in device trust; existing devices get exactly the chat access their current grants imply. Changes apply live: a downgrade stops broadcasts and rejects further chat requests immediately, chat actions still waiting in the queue do not run, results of requests already in flight are withheld, an upgrade applies after the trust write, and the app updates without reconnecting. A queued Stop that loses access also stops canceling new extension dialogs. The app keeps chat access per Mac: switching to a Mac last seen at Off shows it locked, and its cached transcript is dropped, not shown under another Mac's access.

New agent supports explicit project root, existing worktree or deliberate new worktree. New terminal uses scoped daemon Spawn terminal, writes the manifest, and appears in that workspace. Catalog availability is an executable hint using the daemon's login PATH and project-root cwd; a worktree-relative command, wrapper or runtime may still fail. Older daemons lacking additive canonical catalog fields show unsupported launch capability rather than guessed choices.

Each submitted management form keeps its operationId. Reconnect queries workspace.operation and refreshes inventory; it never repeats a spawn, create or terminal request. A daemon commit may outlive a lost receipt or canceled socket, so post-dispatch errors are uncertain. The bridge keeps at most2048 entries without eviction for its lifetime; after restart or re-pair status can be unknown. Unknown is never proof of non-commit and never authorizes automatic replay, even with the original ID. Review authoritative inventory before explicitly launching again. Permission loss removes sensitive result detail and prevents further dispatch; it cannot recall committed work.

Bridge verification requires the isolated lock helper (no install): from repository root run `cargo build -p pu-cli --bin pu-point-guard-lock`, then from this directory set `POINT_GUARD_LOCK_HELPER_PATH` to the absolute worktree `target/debug/pu-point-guard-lock` path for `npm test`. Run `npm run check` separately. Owner Xcode/device checks remain required for the Mac toggle and iPhone UI; do not run app build phases from an agent.
