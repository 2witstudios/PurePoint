# Point Guard standalone verification

The desktop chat adapts the merged mobile bridge v1 model and protocol, with desktop names, separate recovery storage/preferences and a separate Keychain service. Neither UI owns Pi's native sessions or starts/stops the bridge. Pi chat state belongs to AppState and survives view navigation; workspace and shell terminals continue through daemon IPC.

From the repository root:

```sh
(cd apps/purepoint-mobile && npm ci --ignore-scripts)
python3 apps/purepoint-macos/verification/check-point-guard.py
```

This compiles only the Swift domain/model into temporary standalone executables. It uses an ephemeral loopback bridge and deterministic real RPC subprocess, random fixture credential, isolated home/preferences, and no Keychain writes. It never builds the app, replaces installed binaries, calls providers, rewrites native Pi sessions, or creates pairing pages.

The integration checks compile the actual phone and desktop models together and keep both connected. They cover shared state, local drafts, submitter-specific canceled recovery, independent disconnect/reconnect, send/receipt, rich replies/tools, typing during delivery, reconnect without replay, queued follow-up/Stop recovery, native extension selection, read-only history, resume and new conversation. The existing mobile recovery acceptance suite is also compiled against the desktop model to check draft migration, restart recovery, bounded receipt/cancellation retention, failed storage and persistence-before-transmission.

Validation performed during implementation:

- All 85 bridge tests passed; bridge TypeScript check passed.
- Desktop integration and recovery acceptance checks passed.
- Strict concurrency/warnings-as-errors typecheck of the domain, Keychain service and model passed.
- Standalone SwiftUI typecheck of the chat and settings passed using Xcode's Swift compiler/SDK and minimal app-environment stubs. Existing markdown/code components were included.
- AppState and PointGuardView syntax parse and diff whitespace checks passed.

A full app build, XCTest and visual verification remain owner-run in Xcode, per the repository's macOS build-safety rules. Standalone checks do not establish complete target compilation or actual window layout.

Owner acceptance:

1. Build/run the macOS target in Xcode. Open Settings → Point Guard, set the address of the running Pi bridge and its local pairing secret file, and Connect. Keep the phone connected too; both use /v1 and share one live Pi session.
2. Inspect chat/sidebar/composer in light and dark appearance and at the minimum window size. Search, hide/show the sidebar (Command-Shift-S), start/resume history and browse during a run.
3. Send multiline text with Return; use Shift-Return for a newline. Keep typing during delivery. Select prose, copy fenced code and expand running/completed/failed tool output.
4. Scroll up during streaming: output must not pull you away; Latest returns to the live end. Check native confirm/select/input/editor dialogs and suggested draft text.
5. Queue After reply and Steer, then Stop. Restore canceled text alongside a newer draft. Disconnect/reconnect during a reply without resending. Navigate to another workspace and back while work runs.
6. Switch to Shell, run commands, return to chat, and confirm both continue. Verify existing desktop split panes, ordinary shell tabs, file tabs and terminal history still work.
7. Keep both connected and send from either. Race dialog answers/session changes; verify one winner. Queue from both and Stop from the other device; verify canceled recovery goes only to the submitter. Disconnect/reconnect one while the other continues.
