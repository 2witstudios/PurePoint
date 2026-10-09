# Validation evidence — 2026-10-08

- `npm test`: 28/28 passed. Covers core projection/framing, real child RPC, queue/reconnect/controller races and Stop dialog cancellation, native read-only history, authenticated network, full fixture flow, private QR page/payload behavior, image validation/transport/reconnect and isolated published vanilla Pi 1.1.0.
- `npm run check`: passed (`tsc --noEmit`, checkJs).
- `npm audit`: zero vulnerabilities after pinning ws 8.22.0.
- Local vanilla CLI `--version`: 1.1.0. Upstream checkout: ce950d78f424dcaf9f5d6a03ce80ab141130eb1d. Registry package and lockfile verified locally; no global install/link.
- Swift Foundation standalone logic executable: passed. Draft recovery, stale snapshot suppression, fenced-code splitting and endpoint policy.
- `swiftc -typecheck` on ChatDomain / PairingSecret / ChatModel: passed against the Mac SDK, emitting no application binary.
- `swiftc -frontend -parse` on iOS sources/test source: passed (syntax only).
- `plutil -lint` on standalone Xcode project and Info.plist: passed. Shared scheme and all file references checked. No shell build phases, PurePoint references or installed binary destinations.
- App mark: 1024×1024 opaque PNG, exact copy of the existing PurePoint app icon. Light/dark logo assets are byte-for-byte copies of the existing PurePoint artwork. Asset catalog references and JSON verified.
- Visual revision: standalone Swift grouping checks pass for native/live deduplication, contiguous groups, prose ordering and empty error preservation. ChatDomain/PairingSecret/ChatModel typecheck and all iOS source syntax checks pass. XCTest grouping cases are included for owner execution. No rendered screenshot or iOS SDK build of this visual revision was performed.
- Live bridge setup: launched in PurePoint terminal `ag-4q7frv4f` on the Mac’s Tailscale address; authenticated WebSocket sync passed. A private secret was generated under Jono’s explicit revised authorization, and its private QR page was opened locally. Jono subsequently reported that pairing and mobile chat work. This is owner-reported device evidence, not an agent-observed camera test.
- Current live bridge: PurePoint terminal `ag-lkgk6808`, endpoint `ws://100.94.14.74:8787/v1`, working folder `/Users/jono/dev/purepoint` (verified via process cwd). Daemon status succeeds from that folder with inherited builder identity removed. A new native conversation uses the project folder; the previous home-folder conversation remains in native history. The accepted-receipt composer line was removed; Swift syntax check passed and the owner must rebuild to see that UI change. Jono sent a subsequent mobile status report confirming the work is working.
- QR addition: bridge checkJs passed, audit remains clean, Swift Foundation pairing parser checks passed (valid code, public endpoint, short credential, unsupported version, malformed data), and iOS scanner syntax/Info.plist checks passed. Detailed camera permission/cancellation and accessibility cases remain owner-device checks; no iOS SDK compilation was performed by the agent. User reported successful owner build/launch after the Section initializer fix, before this scanner addition.
- Owner OpenRouter authentication and a real model call: passed through the bridge's RPC child with native configuration, provider `openrouter` and model `~anthropic/claude-haiku-latest`. The prompt was accepted as started, returned “Pi Mobile real connection works.”, and settled without abort/error. This smoke test used `--no-session --no-tools`; it did not save a conversation or delegate workers. The alias's underlying model version was not independently verified.

TDD: initial framing/projection, RPC, controller and network suites were run red with absent implementations, then implemented and passed. Further regression tests cover race discoveries and the real native boundary. Tests are colocated in the standalone module; Swift XCTest domain tests ship for owner Cmd+U, with safe standalone execution of the same core behaviors here.

Not independently validated by the agent: Xcode/iOS SDK build or XCTest execution, SwiftUI rendering/screenshots, simulator/device installation, signing, physical-device keyboard/IME/accessibility, actual worker delegation. Owner-reported app launch and successful mobile connection/chat are recorded above; detailed device acceptance remains outstanding. No xcodebuild, PurePoint Swift build, root Cargo build, daemon install/overwrite, deployment, merge, global executable changes were performed. Owner steps and acceptance exercise are in README.

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
7. Authentication: private secret file generated once with 32 cryptographically random bytes or supplied by the owner, timing-safe comparison, atomic first creation without replacement.
8. Integrity: native session files are not rewritten; read-only branch semantics and pinned runtime verified.
9. Logging: no authorization headers, secret contents, RPC transcript or raw stderr are logged; failures display setup guidance.
10. SSRF: app endpoints constrained to tailnet/loopback; no server-side arbitrary URL fetch operation.

Documentation covers interface, setup, constraints and recovery. No blocking finding remains in the safely verified bridge/domain scope; this is a self-review, not an independent review or evidence of iOS/device validation.

## Composer / sidebar / attachments verification — 2026-10-09

- Bridge tests: 28/28 and checkJs pass. Attachment validation was run red before implementation. New coverage checks native image forwarding, malformed/oversized MIME/base64 rejection, idle-only uploads, image-model capability, a real fixture WebSocket-to-RPC image boundary, and reconnect without image replay.
- Real native Pi/OpenRouter/Haiku Latest image prompt passed: `Pi Mobile image transport works.`, stop reason `stop`, no error. Used an ephemeral `--no-session --no-tools` child and a synthetic non-secret PNG fixture; no owner conversation or workers were touched.
- Swift Foundation payload checks pass: image encoding, named text incorporation, attachment/prompt budgets. Existing grouping, drafts and pairing checks pass. XCTest payload/recovery cases are included but not executed here (Command Line Tools SDK has no XCTest module).
- Composer is inset and rounded without the old full-width material strip; history is a left drawer; tools are inline with no filled card or tool icons; resume uses an explicit primary button style with inverse foreground. Owner screenshot informed this change. iOS build, rendering/gestures/keyboard, photo/file import and physical-device attachment delivery are still owner checks.
- At completion of code checks, the phone remained connected to the previous running bridge. Image capability activation requires an idle bridge restart; it was not interrupted blindly.

## Responsiveness fixes — 2026-10-09

Owner reported cold composer taps taking multiple seconds and intermittent frozen interactions. Code inspection found synchronous recovery JSON/file IO, draft persistence on every character, full generic-plus-typed snapshot decoding on the main actor, repeated Markdown parsing and attachment decoding in view bodies.

Changes: asynchronous recovery/Keychain reads; serial, coalesced background persistence with pre-send recovery flush; background snapshot/history decoding and transcript grouping; generation checks after asynchronous decode; cached row projection with equatable row boundaries; background Markdown and image preparation. Canceled-ID persistence only occurs when IDs change. No bridge restart or daemon changes are required for these UI fixes.

- New standalone responsiveness checks ran red before implementing the writer/typed record boundary, then passed. They check off-main execution, 100 rapid pending draft values with latest-value ordering, independent storage keys/flush, snapshot versus receipt decoding, and Markdown prose/code preservation. These tests use only temporary files, not owner draft storage or credentials.
- Existing standalone Swift domain checks pass. ChatDomain/PairingSecret/ChatModel typecheck passes, including `-strict-concurrency=complete`; all iOS Swift syntax checks pass. Bridge tests remain 28/28 and checkJs passes.
- No iOS SDK build, physical-device timing measurement, Instruments recording or rendered UI validation was performed. Owner should rebuild with Cmd+R, stop the debugger, then force-quit and launch from the Home Screen; test the first composer tap, typing while streaming, long-history scrolling, sidebar opening, and an attached photo in both appearances. If pauses remain, use Xcode Product > Profile with the Hangs/Time Profiler instruments while reproducing them. The prior LLDB shared-cache warning may affect attached-debugger startup; it is not confirmed as the cause of this report.

## Cold launch / debugger isolation — 2026-10-09

Owner still reports a roughly minute-long blank launch with the LLDB shared-cache warning and debug-dylib lookup log. The app entry point has no explicit network/recovery wait before creating ChatView. An additional synchronous Keychain read remained in ConnectionView.onAppear; it now runs detached and cannot replace newer manual edits.

Added shared PiMobile-Device scheme: Release Run configuration, empty debugger identifier, PosixSpawn launcher. Original PiMobile debugger/test scheme and owner signing/project edits are preserved. Added static OSLog App-init and chat-appearance markers to identify whether a reported delay precedes app initialization. This isolates a likely debugger/toolchain issue; it does not assert a measured device fix. Apple Developer Tools guidance: https://developer.apple.com/forums/thread/800067.

Both scheme XMLs parse and assertions confirm same target identity, Device Release/no-LLDB launcher, and unchanged original LLDB scheme. Swift source syntax and strict-concurrency Mac SDK model/domain typechecks pass. No Xcode build, launch or device measurement was performed. Owner next step: select PiMobile-Device, phone destination, Cmd+R; compare cold launch and use launch markers/Instruments if it remains slow. No bridge restart, credential reset, cache deletion or native session modifications.

## Compact drawer and keyboard dismissal — 2026-10-09

Owner requested a cleaner, space-efficient history interface and tap-to-dismiss keyboard behavior. Sidebar now has flat system background, compact Chats header with icon controls, inline unfilled search, 44-point single-line title rows, a subtle active-row highlight, and a small branded settings footer. Removed repeated header branding, per-row dates, separate large new-chat row, filled search field, divider and drawer shadow. Search, lazy rendering, refresh, selected-session accessibility, errors, idle resume/busy browsing and backdrop/swipe/escape dismissal remain.

Conversation ScrollView now has a rectangular tap target and simultaneous tap gesture that clears composer focus, plus an accessibility Dismiss keyboard action. The gesture is scoped outside the composer. Existing interactive scroll dismissal remains.

Validation: all standalone iOS source syntax checks and strict-concurrency Mac SDK domain/model typecheck pass; diff whitespace check passes. No iOS SDK build or rendered/device verification was performed. Owner: rebuild PiMobile-Device with Cmd+R, inspect drawer in light/dark and large Dynamic Type, search/select/refresh history, type and tap prose/empty space, then test text-selection long press, tool expansion and code copy while the keyboard is open. No bridge restart is needed. Owner signing/project/workspace edits are preserved.

## Default run scheme — 2026-10-09

Owner still encountered debugging with the usual Run action. PiMobile now defaults to Release Run with no selected debugger and PosixSpawn launcher, matching PiMobile-Device. Original Debug/LLDB launch configuration is preserved as PiMobile-Debug. Debug XCTest configuration remains available with Cmd+U. Scheme XML parses and assertions verify all three Run modes. No Xcode build or actual launch performed; owner must reload the project/scheme if Xcode keeps cached settings and verify Run > Info > Debug executable is unchecked. Owner signing/project/workspace changes remain untouched.

## PurePoint mobile branding — 2026-10-09

App display name, navigation/header/composer/connection/sidebar branding, preview title, Xcode project, app/test targets, executable/test-host references and shared Run schemes are now PurePoint / PurePointTests / PurePoint-Debug / PurePoint-Device. Default Run remains Release without LLDB. Source directory names, existing bundle identifiers, Keychain service, preferences and private recovery-storage namespace remain unchanged to preserve the same installed app and saved pairing/drafts. Existing PurePoint logo/artwork remains.

Owner's project signing/format edits and untracked workspace were preserved in the renamed PurePoint.xcodeproj; only the naming transformation against the committed project was staged. XCTest now imports the renamed PurePoint module. Plist/project syntax, iOS source syntax and Mac SDK domain/model typecheck pass. Parsed project/scheme assertions verify target IDs, names and default non-debug launch. No iOS SDK build/device install was performed. Owner must reopen ios/PurePoint.xcodeproj, select PurePoint and Cmd+R to update the installed name.

## App location — 2026-10-09

Owner superseded the original experiments location: the complete standalone module now lives at apps/purepoint-mobile alongside apps/purepoint-macos. PurePoint.xcodeproj, PurePoint source and PurePointTests are at the module root; standalone Swift check executables' sources are under verification. Bridge, dedicated docs/tests, exact runtime lockfile and local node_modules moved together. Package metadata is named purepoint-mobile; README commands and project links use the new paths.

Preserved local Xcode signing/formatting edits and untracked workspace in the moved project. Only our path transformations against the committed project are staged; bundle IDs and credential/draft storage stay unchanged. A temporary untracked local experiments/pi-mobile symlink targets ../apps/purepoint-mobile so the currently running bridge/Pi child can still resolve old absolute module/dependency paths. It is not a second source tree and is not committed. Remove it after the bridge is safely relaunched from apps/purepoint-mobile; do not interrupt an active Pi run to remove the link.

Verification from apps/purepoint-mobile: bridge tests 28/28, checkJs, Swift domain and responsiveness checks, iOS Swift source parsing, Mac SDK domain/model typecheck and plist/project validation pass. Parsed schemes resolve unchanged target IDs; Swift file references resolve under the new source folders; the compatibility link resolves to the moved bridge. No Xcode build, signing, launch, daemon binary overwrite or bridge interruption was performed. Owner: close the former Xcode window, open apps/purepoint-mobile/PurePoint.xcodeproj, select PurePoint and Cmd+R.

## Remove on-device preview — 2026-10-09

Removed the Preview section/action from Connection settings under owner direction; updated the README to distinguish developer-only Xcode preview from the real app. The fixture bridge and SwiftUI #Preview remain for deterministic development. Swift source syntax checks pass. No Xcode build/device run performed; owner rebuilds with PurePoint Cmd+R. Signing/workspace edits preserved.

## Header connection status — 2026-10-09

Moved connection status from the conditional composer footer into the existing header subtitle. The header uses connectionStatus while disconnected (including connecting/reconnect failure), Connected while connected/idle, and Working while connected/busy; cached busy state cannot mask disconnection. Always-present, single-line subtitle keeps header structure stable. Header now opens Connection settings with a plain style and accessible hint, replacing the former footer shortcut. No bridge or persistence changes.

All iOS Swift sources parse; Mac SDK domain/model typecheck and diff whitespace checks pass. No iOS SDK compilation/render/device run performed. Owner rebuilds PurePoint Cmd+R; inspect composer position while reconnecting and test the header's connection shortcut.
