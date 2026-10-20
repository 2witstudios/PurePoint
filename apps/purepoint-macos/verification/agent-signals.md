# Agent signal consumer verification

Producer: `e9d8b6c4ea2f36682e23ec46c7b41c082b5d873a` (CORE), fixtures unchanged from early contract `9216ac5e82b545de34399237de0b8e8b40ef6c19`.

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

No adapter is release-qualified by default. The local notification service requires the base64 JSON `[tool, adapter_version]` string (tool releases update freely; qualification follows the adapter) in the owner-controlled `PP_signalQualifiedMacAdapters` UserDefaults array. Obtain that value from `AgentSignalSnapshot.qualificationKey` for the tested producer; do not fabricate a tool identity. Permission/question delivery additionally requires the selected authoritative request's producer `actionability.macos == true`. Error/completion delivery also requires the adapter release gate. User notification and sound preferences are separate toggles in existing General settings, initially off. Replaying snapshots never enables a capability or sends terminal input.

Before enabling a combination, the owner must build/run the composed candidate in Xcode and record: actual tool/version/adapter and existing execution mode; agent launch/native-session identity; a specific unresolved request visible in its existing terminal; background the app; observe a generic native notification and optional sound; tap; verify current status is fetched and the correct existing agent tab is selected; discover that exact request; respond through existing terminal input; verify the producer resolves that request and withdraws badge/notification. Repeat allow/deny or answer/cancel, multiple requests, root/child, new launch, offline/restart, and resolution before tap. No new prompt editor or notification answer buttons are provided. Do not enable when the request cannot be discovered and answered in the existing interface. No supported Mac fallback is advertised to iOS until that route has passed this test.

Owner Xcode/device checks NOT RUN here: complete app typecheck/link and XCTest; narrow/long-name/selected row appearance; unchanged 24-point Mac rows; Dynamic Type/VoiceOver; keyboard and terminal input; native notification authorization, badge, sound, foreground suppression, tap and restart behavior. Focused source parsing/typechecking does not replace these checks. Viewing an error in the foreground acknowledges its Mac badge while retaining its Error pill; policy/defaults require owner design acceptance. No shared cross-device acknowledgment is claimed.

## Foundation dependency

Present: macOS DaemonLifecycle and ProjectState status subscription; app-owned daemon shutdown in AppState; mobile ChatModel WebSocket, AgentControlModel foreground inventory and existing terminal controls. Missing: independent shared service acceptance, APNs registration/token lifecycle/entitlements, authenticated push transport/device revocation, durable service outbox/APNs provider and signing/device proof. None is implemented here. iOS badges reconcile only while the app runs in foreground; suspension clears the locally projected badge as stale. Locked/backgrounded/terminated iOS push remains unsupported and scope-gated. A desktop/foreground-phone consumer PR is partial delivery, not full plan acceptance.

## Independent review corrections

The initial snapshot review requested changes for scope loss in notification navigation and false presence behind Settings. Notifications now retain project (including machine), worktree, agent, launch, generation, native session and event through current-status revalidation and existing tab/leaf selection. Routing requires exactly one scoped surface and an unambiguous global workspace identity; duplicate adopted workspace IDs fail closed. Existing spawn selection remains unchanged. Error acknowledgment/foreground suppression requires the actual visible selected workspace, focused agent, active app/key window and Settings closed; library navigation does not count as terminal presence.

`SignalRoutingChecks.swift` covers same agent ID on local/remote roots, absent/duplicate targets, cross-project and same-project/worktree workspace ID collisions, launch generation/session/worktree/expiry revalidation, hidden Settings/library presence and unchanged unseen Error badge. Owner acceptance must also exercise these cases through the built app; source tests do not satisfy that device gate.

The next review confirmed a launch-time notification response could arrive before the lazy service/delegate and in-memory routes existed. The application delegate now installs the shared notification delegate during `applicationWillFinishLaunching`. A pending tap queue keeps at most 16 opaque keys for at most 120 seconds, with an 8192-byte key bound, deduplication and clock rollback rejection. Status/project restoration can reconstruct the exact route; only successful current-status validation and scoped navigation consumes the tap. Failed fetches retry on the existing signal reconciliation tick until bounded expiry. No request input is queued or replayed. `SignalPendingTapChecks.swift` tests loading gaps, fetch retry, success consumption and bounds. Owner must additionally test notification-triggered cold launch/relaunch before project restoration, unavailable host, and request resolution before restoration. Full native cold-launch/device behavior is NOT RUN here.

## Proof-backed producer activation

The separately owned ENGINE producer supports an owner-authorized, per-project `.pu/agent-signals/qualifications.json` file: private regular owner file, at most 16 KiB, schema version 1, exact tool/adapter_version/platform `macos` (`tool_version` is recorded but not matched), `owner_confirmed: true` and a nonempty proof-record URL. ENGINE validates this fail-closed at launch and only applies it to authoritative pending requests. Owner writes the record only after the native acceptance chain above is recorded; a new agent launch is required. The source producer SHA and exact config format are pinned in the PageSpace handoff once its independent review is complete. No record is created by these checks and iOS actionability stays false.

The native release gate is separate: producer evidence tuple `["pi","1.3.0","1"]` maps exactly to `WyJwaSIsIjEuMy4wIiwiMSJd` in the `PP_signalQualifiedMacAdapters` UserDefaults string array. Owner release qualification plus the existing General notification/sound toggles and OS authorization enables that accepted combination. The native consumer never infers qualification from a proof URL or changes producer config itself. This two-gate path is implemented but remains disabled pending real owner evidence.
