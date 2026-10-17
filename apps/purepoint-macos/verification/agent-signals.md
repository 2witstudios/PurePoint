# Agent signal consumer verification

Producer: `f9a91c595d175d7ba3fc214fec8c6527d0d1a305` (CORE), fixtures unchanged from early contract `9216ac5e82b545de34399237de0b8e8b40ef6c19`.

Run from the repository root:

```sh
sh apps/purepoint-macos/verification/check-signals.sh
sh apps/purepoint-mobile/verification/check-agent-control.sh
sh apps/purepoint-mobile/verification/check-navigation.sh
npm --prefix apps/purepoint-mobile run check
# Build only the isolated test lock helper, never install or overwrite a daemon:
cargo build -p pu-cli --bin pu-point-guard-lock
POINT_GUARD_LOCK_HELPER_PATH="$PWD/target/debug/pu-point-guard-lock" npm --prefix apps/purepoint-mobile test
python3 apps/purepoint-macos/verification/check-pi-terminal.py --pi /path/to/isolated/pi-1.3.0
```

The Swift checks compile focused models/protocol/notification code, without an app build or app installation. They decode the actual Rust fixtures through the macOS DaemonResponse and phone ControlledAgent consumers. The phone badge test uses isolated UserDefaults and a foreground inventory fixture. The Node bridge test forwards the exact producer snapshot without altering readiness or input routing. Its lock helper is required by the existing test suite; omission produces `lock_helper_missing`, not an implementation failure.

The PTY harness uses an explicit temporary extension, a fresh HOME and PI_CODING_AGENT_DIR, no provider credentials, and an awaited input command. pi 1.3.0, source pin `8efd314fa3d3232ce197d8d7a6a5660530b9d83e`: PASS exact question text displayed on a real interactive PTY, typed answer returned, native session identity, ordered ui_prompt_start/end. This is a question, not permission. It does not run the PurePoint app, its daemon adapter or notification tap. It does not qualify macOS/iOS actionability. Startup readiness is awaited before sending /probe.

## Release capability path and owner gate

No adapter is release-qualified by default. The local notification service requires the exact base64 JSON `[tool, tool_version, adapter_version]` string in the owner-controlled `PP_signalQualifiedMacAdapters` UserDefaults array. Obtain that value from `AgentSignalSnapshot.qualificationKey` for the tested producer; do not fabricate a tool identity. Permission/question delivery additionally requires the selected authoritative request's producer `actionability.macos == true`. Error/completion delivery also requires the adapter release gate. User notification and sound preferences are separate toggles in existing General settings, initially off. Replaying snapshots never enables a capability or sends terminal input.

Before enabling a combination, the owner must build/run the composed candidate in Xcode and record: actual tool/version/adapter and existing execution mode; agent launch/native-session identity; a specific unresolved request visible in its existing terminal; background the app; observe a generic native notification and optional sound; tap; verify current status is fetched and the correct existing agent tab is selected; discover that exact request; respond through existing terminal input; verify the producer resolves that request and withdraws badge/notification. Repeat allow/deny or answer/cancel, multiple requests, root/child, new launch, offline/restart, and resolution before tap. No new prompt editor or notification answer buttons are provided. Do not enable when the request cannot be discovered and answered in the existing interface. No supported Mac fallback is advertised to iOS until that route has passed this test.

Owner Xcode/device checks NOT RUN here: complete app typecheck/link and XCTest; narrow/long-name/selected row appearance; unchanged 24-point Mac rows; Dynamic Type/VoiceOver; keyboard and terminal input; native notification authorization, badge, sound, foreground suppression, tap and restart behavior. Focused source parsing/typechecking does not replace these checks. Viewing an error in the foreground acknowledges its Mac badge while retaining its Error pill; policy/defaults require owner design acceptance. No shared cross-device acknowledgment is claimed.

## Foundation dependency

Present: macOS DaemonLifecycle and ProjectState status subscription; app-owned daemon shutdown in AppState; mobile ChatModel WebSocket, AgentControlModel foreground inventory and existing terminal controls. Missing: independent shared service acceptance, APNs registration/token lifecycle/entitlements, authenticated push transport/device revocation, durable service outbox/APNs provider and signing/device proof. None is implemented here. iOS badges reconcile only while the app runs in foreground; suspension clears the locally projected badge as stale. Locked/backgrounded/terminated iOS push remains unsupported and scope-gated. A desktop/foreground-phone consumer PR is partial delivery, not full plan acceptance.
