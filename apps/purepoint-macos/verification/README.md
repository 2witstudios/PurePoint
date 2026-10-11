# Point Guard standalone verification

The desktop chat adapts the merged mobile bridge v1 model and protocol, with desktop names, separate recovery storage/preferences and a separate Keychain service. Point Guard uses the standard pane grid as a folder-root workspace using Pi’s configured working folder (home by default): chat, terminals, agents and files are tabs and splits, without Chat/Shell modes. Neither UI owns Pi's native sessions or starts/stops the bridge. Pi chat state belongs to AppState and survives view navigation; workspace and shell terminals continue through daemon IPC.

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

Point Guard pane checks can run without the app build phases:

```sh
DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer python3 apps/purepoint-macos/verification/check-point-guard-workspace.py
```

The temporary package runs the existing pane stack, workspace reconciliation, model and grid-persistence tests plus Point Guard workspace tests against the actual registry and persistence code. File-editor storage and external app services/lifecycle are stubbed; these checks do not start a daemon, spawn processes, test the palette UI, or replace installed binaries. The production AppState, surface spawn method and daemon protocol also run in the isolated harness. The production PointGuardView also typechecks against stub rendering dependencies, including its observation of folder changes; this does not establish actual window behavior. Routing checks cover both split directions, shell/built-in/custom spawn request serialization, independent project selection, no open project, working-folder changes, file roots, agent lookup and promoting/demoting a Point Guard folder in the project sidebar. The Point Guard checks cover root-only agent placement, closing/reopening chat, fresh IDs after closing the last tab, retained focus, distinct project layouts, restart persistence and isolation after copying or renaming a project directory. Restored Point Guard IDs and the persisted active workspace are re-keyed to the current project path while preserving the pane layout. Identity-only migration also preserves newer schema versions and unknown fields; ordinary layout saves still refuse newer documents. If identity migration cannot be written, the registry publishes no layout for that project, shows a retryable error and retries after storage becomes writable. Permission and disk-full failures are injected in the isolated tests. Retry immediately reconciles the latest manifest received during a failed load, removing stale source-project agent tabs and adopting current destination agents.

Owner acceptance:

The nested conversation sidebar has a standalone rendered layout check:

```sh
DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer python3 apps/purepoint-macos/verification/check-point-guard-sidebar.py
```

It hosts the actual sidebar layout and pane splitter inside a native `NavigationSplitView`,
with a placeholder navigation column. It clicks the real SwiftUI toggle at the history
sidebar's top-right corner and on its collapsed rail, and measures history/transcript
bounds across repeated open/close cycles, narrow widths and sibling splits. It briefly
opens an isolated test window and uses no daemon or app build phases.
It also compiles the production chat model, chat view, workspace view and pane controls
to verify local controls and shared-preference shortcuts with two visible chats, a
focused terminal/file sibling, hidden chat tabs and the no-project fallback. The daemon
environment/setup sheets are stubbed; Pi stays disconnected and storage is isolated.
In Xcode, also verify that the toggle stays at the history sidebar's top-right corner
and remains available on its collapsed rail, that dragging the divider resizes history,
and that Command-Shift-S toggles once with multiple chats or a terminal/file sibling focused.

1. Build/run the macOS target in Xcode. Open Settings → Point Guard, set the address of the running Pi bridge and its local pairing secret file, and Connect. Keep the phone connected too; both use /v1 and share one live Pi session.
2. Inspect chat/sidebar/composer in light and dark appearance and at the minimum window size. Search, hide/show the sidebar (Command-Shift-S), start/resume history and browse during a run.
3. Send multiline text with Return; use Shift-Return for a newline. Keep typing during delivery. Select prose, copy fenced code and expand running/completed/failed tool output.
4. Scroll up during streaming: output must not pull you away; Latest returns to the live end. Check native confirm/select/input/editor dialogs and suggested draft text.
5. Queue After reply and Steer, then Stop. Restore canceled text alongside a newer draft. Disconnect/reconnect during a reply without resending. Navigate to another workspace and back while work runs.
6. In Point Guard, use Cmd+N, Cmd+T, the tab strip plus and both split controls. Choose Terminal, a built-in agent, a custom agent command and Files. Keep a different coding project selected, then verify they run at Pi’s working folder (for example /Users/jono) outside worktrees and share the Point Guard grid without duplicate sidebar rows. Switch tabs, move tabs between panes, navigate away and back, and restart; chat drafts, terminal scrollback, file edits and layout should survive. Close chat without stopping Pi; closing the final tab returns to chat. Change Pi’s working folder and repeat the split checks; the new panes should use the new folder. Verify ordinary worktree pane behavior still works.
7. Keep both connected and send from either. Race dialog answers/session changes; verify one winner. Queue from both and Stop from the other device; verify canceled recovery goes only to the submitter. Disconnect/reconnect one while the other continues.

The engine process check runs with a separate Cargo target directory and temporary non-Git folder:

```sh
cargo test -p pu-engine --test integration given_point_guard_folder_without_git_should_spawn_split_agents_in_that_folder --target-dir /tmp/purepoint-pg-split-target
```

It uses a shell fixture and a configured fixture agent to record their actual working directories; it does not invoke AI providers or replace installed binaries.
