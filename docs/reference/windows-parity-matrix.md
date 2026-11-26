# Windows Parity Matrix

Maps every macOS app source file to its planned counterpart in the Windows app (`apps/purepoint-windows`, Tauri 2 + React + TypeScript; DEC-7). The plan is the WIN epic; each WIN leaf moves its rows to `ported` in the PR that ports them.

- Baseline: `b275081`. Source: `apps/purepoint-macos/purepoint-macos/` (256 Swift files) and `apps/purepoint-macos/purepoint-macosTests/` (69 test files).
- Windows paths are relative to `apps/purepoint-windows/`: `ui/src/{protocol,model,state,services,views,theme}` for the front end, `src-tauri/src/...` for the Rust backend. They are planned locations; the porting leaf may refine them and updates this file in the same PR.
- Status is `todo`, `ported` or `excluded`. Excluded rows have no leaf and must give a reason: the `+iOS` files and whole-file `#if os(iOS)` files (iPad build only) and the DEC-11 exclusions. Every other row names exactly one WIN leaf.
- `node scripts/check-parity-matrix.mjs` fails when a Swift file, a test file or an SF Symbol in the source is missing here, or a non-excluded row lacks a leaf. `--no-todo` also fails on any `todo` row (the WIN-10.5 and WIN-11.1 gate).
- Keep the tables in this exact format: one row per line, cells separated by `|`, file names in backticks.
- Shared sources outside the macOS app are not rows here. `apps/shared/ChatMarkdown/` (Point Guard chat Markdown: `ChatMarkdown.swift`, `ChatMarkdownView.swift` and the cases in `ChatMarkdownTests.swift`, run by `check-markdown.py`, plus the render-script fixtures) is ported by WIN-9.6 to `ui/src/model/chatMarkdown.ts` and `ui/src/views/chat/ChatMarkdown.tsx`, with tests in `ui/src/model/chatMarkdown.test.ts` and `ui/src/views/chat/ChatMarkdown.test.tsx`.

Summary: run `node scripts/check-parity-matrix.mjs` for live counts (kept out of this file so parallel leaves don't conflict).

## Source files

| Swift file | Lines | Windows location | Leaf | Status | Exclusion reason |
|---|---|---|---|---|---|
| `AppCommands.swift` | 404 | `ui/src/views/shell/appCommands.ts`, `ui/src/views/shell/MenuBar.tsx` (in-app menu bar in the title bar) | WIN-7.1 | ported | — |
| `ContentView.swift` | 364 | `ui/src/views/shell/ContentView.tsx`, `ui/src/views/shell/bootstrap.ts`, `ui/src/views/shell/sidebarWidth.ts` | WIN-7.1 | ported | — |
| `Models/AgentSignalPendingTaps.swift` | 21 | `ui/src/model/agentSignalPendingTaps.ts` | WIN-7.7 | todo | — |
| `Models/AgentSignalPolicy.swift` | 59 | `ui/src/model/agentSignalPolicy.ts` | WIN-7.7 | todo | — |
| `Models/AgentSignalRouting.swift` | 59 | `ui/src/model/agentSignalRouting.ts` | WIN-7.7 | todo | — |
| `Models/AgentSignalSnapshot.swift` | 85 | `ui/src/model/agentSignalSnapshot.ts` | WIN-7.7 | todo | — |
| `Models/AgentStatus.swift` | 48 | `ui/src/model/agentStatus.ts` | WIN-6.0 | ported | — |
| `Models/AgentVariant.swift` | 73 | `ui/src/model/agentVariant.ts` | WIN-6.6 | ported | — |
| `Models/AgentsHubModels.swift` | 160 | `ui/src/model/agentsHubModels.ts` | WIN-8.7 | todo | — |
| `Models/ChannelModel.swift` | 58 | `ui/src/model/channelModel.ts` | WIN-8.10 | todo | — |
| `Models/ChatMessage.swift` | 56 | `ui/src/model/content.ts` | WIN-8.16 | todo | — |
| `Models/CommandPaletteItem.swift` | 180 | `ui/src/model/commandPalette.ts` | WIN-6.6 | ported | — |
| `Models/ContentBlockSplitter.swift` | 81 | `ui/src/model/content.ts` | WIN-8.16 | todo | — |
| `Models/Conversation.swift` | 47 | `src-tauri/src/conversations/model.rs` | WIN-8.12 | ported | — |
| `Models/DiffModel.swift` | 180 | `ui/src/model/diff.ts` (review data types are `ui/src/services/git.ts`, decoded in Rust by WIN-8.1) | WIN-6.0 | ported | — |
| `Models/EditorTab.swift` | 142 | `ui/src/model/editorTab.ts` | WIN-8.5 | todo | — |
| `Models/FileNode.swift` | 38 | `ui/src/model/fileNode.ts` | WIN-8.4 | todo | — |
| `Models/KeyBinding+iOS.swift` | 128 | — | — | excluded | `+iOS` file (iPad build only) |
| `Models/KeyBinding.swift` | 449 | `ui/src/model/keyBinding.ts` | WIN-6.7 | ported | — |
| `Models/Machine.swift` | 278 | `ui/src/model/machine.ts` | WIN-8.14 | todo | — |
| `Models/ManifestModel.swift` | 100 | `ui/src/protocol/generated/` (replaced by generated types; Swift's lenient decoding in `ui/src/services/daemonConnection.ts`) | WIN-6.0 | ported | — |
| `Models/PRModel.swift` | 33 | `ui/src/model/pr.ts` (`PullRequestInfo` from `ui/src/services/git.ts`) | WIN-6.0 | ported | — |
| `Models/PiChatDomain.swift` | 368 | `ui/src/model/piChatDomain.ts` | WIN-9.4 | todo | — |
| `Models/PointGuardPlays.swift` | 219 | `ui/src/model/pointGuardPlays.ts` | WIN-8.13 | todo | — |
| `Models/PointGuardRuntime.swift` | 96 | `ui/src/model/pointGuardRuntime.ts` | WIN-8.13 | todo | — |
| `Models/ProjectKey.swift` | 159 | `ui/src/model/projectKey.ts` | WIN-6.5 | ported | — |
| `Models/ProjectOrder.swift` | 67 | `ui/src/model/projectOrder.ts` | WIN-6.5 | ported | — |
| `Models/ScheduleEvent.swift` | 231 | `ui/src/model/scheduleEvent.ts` | WIN-8.8 | todo | — |
| `Models/SidebarItem.swift` | 136 | `ui/src/model/sidebarItem.ts` | WIN-6.5 | ported | — |
| `Models/StreamEvent.swift` | 122 | `src-tauri/src/conversations/stream_event.rs` | WIN-8.12 | ported | — |
| `Models/TriggerItem.swift` | 59 | `ui/src/model/triggerItem.ts` | WIN-8.9 | todo | — |
| `Models/Workspace.swift` | 594 | `ui/src/model/workspace.ts`, `ui/src/model/paneSplit.ts` (the tree operations it uses) | WIN-6.1 | ported | — |
| `Models/WorkspaceModel.swift` | 73 | `ui/src/model/workspaceModel.ts` | WIN-6.1 | ported | — |
| `Platform/AppActivity+iOS.swift` | 97 | — | — | excluded | `+iOS` file (iPad build only) |
| `Platform/AppKitShims+iOS.swift` | 81 | — | — | excluded | `+iOS` file (iPad build only) |
| `Platform/AppLifecycleHooks.swift` | 27 | `ui/src/services/appLifecycle.ts` | WIN-8.15 | todo | — |
| `Platform/HSplitView+iOS.swift` | 25 | — | — | excluded | `+iOS` file (iPad build only) |
| `Platform/LocalFilePresenter+iOS.swift` | 113 | — | — | excluded | `+iOS` file (iPad build only) |
| `Platform/MacControlStyles+iOS.swift` | 299 | — | — | excluded | `+iOS` file (iPad build only) |
| `Platform/MacTextStyles+iOS.swift` | 22 | — | — | excluded | `+iOS` file (iPad build only) |
| `Platform/NSApplication+iOS.swift` | 39 | — | — | excluded | `+iOS` file (iPad build only) |
| `Platform/PlatformTypes.swift` | 28 | `ui/src/model/platformTypes.ts` | WIN-6.0 | ported | — |
| `PurePointiPadApp.swift` | 67 | — | — | excluded | Whole-file `#if os(iOS)` (iPad app entry point) |
| `Services/AgentSignalNotifications.swift` | 173 | `ui/src/services/notifications.ts` | WIN-7.7 | todo | — |
| `Services/AttachmentUpload.swift` | 102 | `ui/src/services/attachmentUpload.ts` | WIN-9.7 | todo | — |
| `Services/BundleSeal.swift` | 52 | `src-tauri/src/pointguard/seal.rs` | WIN-9.3 | todo | — |
| `Services/CLIInstaller.swift` | 215 | `src-tauri/src/installer/cli.rs`, `src-tauri/src/installer/user_path.rs`, `src-tauri/src/installer/mod.rs`, `ui/src/services/installer.ts` | WIN-10.1 | ported | — |
| `Services/CheckForUpdatesViewModel.swift` | 25 | `ui/src/services/updater.ts` (tauri-plugin-updater) | WIN-10.3 | todo | — |
| `Services/ClaudeConversationIndex.swift` | 497 | `src-tauri/src/conversations/claude.rs`, `src-tauri/src/conversations/util.rs`, `ui/src/services/conversations.ts` | WIN-8.12 | ported | — |
| `Services/ClaudeProcess.swift` | 168 | — | — | excluded | DEC-11: legacy chat; WIN-8.12 grep found no caller outside `State/ChatState.swift` (which declares `ClaudeProcessProvider`) and `ClaudeProcessTests`, and nothing constructs it |
| `Services/CodexConversationIndex.swift` | 160 | `src-tauri/src/conversations/codex.rs` | WIN-8.12 | ported | — |
| `Services/DaemonAttachSession.swift` | 354 | `src-tauri/src/attach/session.rs`, `src-tauri/src/attach/mod.rs`, `ui/src/services/attach.ts` | WIN-5.4 | ported | — |
| `Services/DaemonClient.swift` | 138 | `src-tauri/src/daemon/client.rs`, `src-tauri/src/daemon/relay.rs`, `ui/src/services/daemon.ts` | WIN-5.3 | ported | — |
| `Services/DaemonConnection.swift` | 80 | `ui/src/services/daemonConnection.ts` | WIN-6.0 | ported | — |
| `Services/DaemonGridSubscription.swift` | 127 | `src-tauri/src/subscriptions/grid.rs`, `ui/src/state/gridCommands.ts` | WIN-6.4 | todo | WIN-5.5 ported the stream (`grid.rs`: same backoff, 20 retries, commands on the subscription connection; `grid-event {projectRoot, command}`, `subscriptions_grid_command`); WIN-6.4 applies commands to the registry (`handleRemoteCommand`) |
| `Services/DaemonLifecycle.swift` | 285 | `src-tauri/src/daemon/lifecycle.rs` | WIN-5.3 | ported | — |
| `Services/DaemonProjectsSubscription.swift` | 69 | `src-tauri/src/subscriptions/projects.rs`, `ui/src/services/subscriptions.ts` | WIN-5.5 | ported | 0.5 s doubling to 10 s, never gives up; `projects-changed {projects}`; `subscriptions_start_projects` ensures the daemon first (`followLocalProjects`) |
| `Services/DaemonProtocol.swift` | 1438 | `ui/src/protocol/generated/` (from pu-core via ts-rs) | WIN-5.2 | todo | — |
| `Services/DaemonStatusSubscription.swift` | 143 | `src-tauri/src/subscriptions/status.rs`, `ui/src/services/subscriptions.ts` | WIN-5.5 | ported | Per project root; 0.5 s doubling to 5 s, `BUSY` retried, `subscription-state` `daemon_lost` on a dropped stream or the 4th failure; `status-event` carries the raw payload (AgentModel mapping goes with WIN-6.0); no iOS foreground handling |
| `Services/DaemonTransport.swift` | 197 | `src-tauri/src/daemon/client.rs` (replaces the Swift transport), `ui/src/services/daemonConnection.ts` (`DaemonTransport` seam, `DaemonRequester` per endpoint) | WIN-6.0 | ported | — |
| `Services/DaemonWorkspaceService.swift` | 73 | `ui/src/services/workspaceService.ts` | WIN-6.0 | ported | — |
| `Services/FileIOService.swift` | 175 | `src-tauri/src/git/file_io.rs`, `ui/src/services/fileIO.ts` | WIN-8.5 | todo | — |
| `Services/FileIndex.swift` | 52 | `src-tauri/src/files/index.rs`, `ui/src/services/fileIndex.ts` | WIN-6.6 | ported | — |
| `Services/FileTreeWatcher.swift` | 96 | `src-tauri/src/watchers/file_tree.rs` | WIN-5.6 | ported | One recursive `notify` watch per root (not one source per expanded dir), skipping `FileTreeState` hidden names; `file-tree-changed {root, paths}` via `ui/src/services/watchers.ts` |
| `Services/GitService.swift` | 891 | `src-tauri/src/git/{service,review,pr,patch,runner,model,commands}.rs`, `ui/src/services/git.ts` | WIN-8.1 | ported | — |
| `Services/HotkeyMonitor+iOS.swift` | 136 | — | — | excluded | `+iOS` file (iPad build only) |
| `Services/HotkeyMonitor.swift` | 49 | `ui/src/services/hotkeys.ts` (DOM key handling) | WIN-7.6 | todo | — |
| `Services/MachineConnection+iOS.swift` | 380 | — | — | excluded | `+iOS` file (iPad build only) |
| `Services/MachineConnection.swift` | 650 | `src-tauri/src/daemon/machine.rs` | WIN-8.14 | todo | — |
| `Services/MachineRegistryWatcher.swift` | 146 | `src-tauri/src/watchers/machine_registry.rs` | WIN-5.6 | ported | Watches the parent dir by name (no re-open after rename), 1 s retry while absent; `machines-changed` |
| `Services/ManifestWatcher.swift` | 116 | `src-tauri/src/watchers/manifest.rs` | WIN-5.6 | ported | Watches `.pu` by name, 50 ms debounce, one event per rename burst; `manifest-changed {projectRoot}`; WIN-3.13 release in `watchers/mod.rs` |
| `Services/MarkdownHTMLRenderer.swift` | 245 | `ui/src/services/markdownHtml.ts` | WIN-8.6 | todo | — |
| `Services/NSView+Constraints+iOS.swift` | 18 | — | — | excluded | `+iOS` file (iPad build only) |
| `Services/NSView+Constraints.swift` | 18 | `ui/src/views/shell/layout.css` (CSS layout replaces Auto Layout pinning) | WIN-7.1 | ported | — |
| `Services/OpenCodeConversationIndex.swift` | 116 | `src-tauri/src/conversations/opencode.rs` | WIN-8.12 | ported | — |
| `Services/PiAttachmentImport.swift` | 72 | `ui/src/services/attachmentImport.ts` | WIN-9.7 | todo | — |
| `Services/PiPairedMac+iOS.swift` | 225 | — | — | excluded | `+iOS` file (iPad build only) |
| `Services/PiPairingSecret.swift` | 41 | `src-tauri/src/secrets/pairing.rs` (Credential Manager) | WIN-9.7 | todo | — |
| `Services/PointGuardApplicationDelegate.swift` | 51 | `src-tauri/src/pointguard/lifecycle.rs` | WIN-9.3 | todo | — |
| `Services/PointGuardTailnet.swift` | 81 | `src-tauri/src/pointguard/tailnet.rs` | WIN-9.3 | todo | — |
| `Services/ProjectHost.swift` | 719 | `src-tauri/src/git/host.rs` (`GitExec` daemon client wired by WIN-5.3; directory watchers live with `watchers/` and `subscriptions/`) | WIN-8.1 | ported | — |
| `Services/SSH/CitadelSSH.swift` | 257 | — | — | excluded | Whole-file `#if os(iOS)`; Citadel SSH, replaced by system `ssh.exe` (DEC-5, DEC-11) |
| `Services/SSH/DebugLaunchArguments+iOS.swift` | 214 | — | — | excluded | `+iOS` file (iPad build only) |
| `Services/SSH/HostKeyPinStore.swift` | 142 | — | — | excluded | Whole-file `#if os(iOS)`; Citadel SSH, replaced by system `ssh.exe` (DEC-5, DEC-11) |
| `Services/SSH/SSHExecTransport.swift` | 289 | — | — | excluded | Whole-file `#if os(iOS)`; Citadel SSH, replaced by system `ssh.exe` (DEC-5, DEC-11) |
| `Services/SSH/SSHIdentityStore.swift` | 148 | — | — | excluded | Whole-file `#if os(iOS)`; Citadel SSH, replaced by system `ssh.exe` (DEC-5, DEC-11) |
| `Services/SSH/SSHKeyFormat.swift` | 265 | — | — | excluded | Whole-file `#if os(iOS)`; Citadel SSH, replaced by system `ssh.exe` (DEC-5, DEC-11) |
| `Services/SSH/SSHMachineSession.swift` | 365 | — | — | excluded | Whole-file `#if os(iOS)`; Citadel SSH, replaced by system `ssh.exe` (DEC-5, DEC-11) |
| `Services/SSH/SSHMachineSupport.swift` | 35 | — | — | excluded | Whole-file `#if os(iOS)`; Citadel SSH, replaced by system `ssh.exe` (DEC-5, DEC-11) |
| `Services/ShellUtilities.swift` | 15 | `ui/src/model/shellEscape.ts`, `src-tauri/src/conversations/util.rs` (`parse_json_line`) | WIN-6.0 | ported | — |
| `Services/SyntaxHighlightManager.swift` | 513 | `ui/src/services/syntaxHighlight.ts` | WIN-8.5 | todo | — |
| `Services/TranscriptParser.swift` | 222 | `src-tauri/src/conversations/transcript.rs` | WIN-8.12 | ported | — |
| `Services/UnixSocketTransport.swift` | 204 | `src-tauri/src/daemon/client.rs` (pu-transport named pipe) | WIN-6.0 | ported | — |
| `Services/WorkspaceService.swift` | 13 | `ui/src/services/workspaceService.ts` | WIN-6.0 | ported | — |
| `Services/WorktreeWatcher.swift` | 69 | `src-tauri/src/watchers/worktree.rs` | WIN-5.6 | ported | Same metadata paths (`.git`, HEAD, index, packed-refs, refs), 500 ms debounce; `worktrees-changed {worktreePath}` |
| `State/AddMachineFlow+iOS.swift` | 313 | — | — | excluded | `+iOS` file (iPad build only) |
| `State/AgentConfigState.swift` | 243 | `ui/src/state/agentConfigState.ts` | WIN-8.7 | todo | — |
| `State/AgentsHubState.swift` | 270 | `ui/src/state/agentsHubState.ts` | WIN-8.7 | todo | — |
| `State/AppState+LibraryScope+iOS.swift` | 123 | — | — | excluded | `+iOS` file (iPad build only) |
| `State/AppState+PointGuardPlays.swift` | 20 | `ui/src/state/appStatePointGuardPlays.ts` | WIN-8.13 | todo | — |
| `State/AppState.swift` | 952 | `ui/src/state/appState.ts` | WIN-6.0 | ported | — |
| `State/ChannelState.swift` | 298 | `ui/src/state/channelState.ts` | WIN-8.10 | todo | — |
| `State/ChatState.swift` | 161 | — | — | excluded | DEC-11: legacy chat (iPad DEC-9) |
| `State/CommandPaletteOpenCoordinator.swift` | 18 | `ui/src/state/commandPaletteOpenCoordinator.ts` | WIN-6.6 | ported | — |
| `State/DiffState.swift` | 281 | `ui/src/state/diffState.ts` | WIN-6.0 | ported | — |
| `State/EditorState.swift` | 263 | `ui/src/state/editorState.ts` | WIN-8.5 | todo | — |
| `State/FileTabStore.swift` | 62 | `ui/src/state/workspaceRegistry.ts` | WIN-6.2 | ported | — |
| `State/FileTreeState.swift` | 208 | `ui/src/state/fileTreeState.ts` | WIN-8.4 | todo | — |
| `State/KeyBindingState.swift` | 138 | `ui/src/state/keyBindingStore.ts` | WIN-6.7 | ported | — |
| `State/MachineState+OpenProject.swift` | 67 | `ui/src/state/machineStateOpenProject.ts` | WIN-8.14 | todo | — |
| `State/MachineState.swift` | 515 | `ui/src/state/machineState.ts` | WIN-8.14 | todo | — |
| `State/PiChatModel.swift` | 827 | `ui/src/state/piChat.ts` | WIN-9.4 | todo | — |
| `State/PointGuardServiceModel.swift` | 516 | `src-tauri/src/pointguard/service.rs`, `ui/src/state/pointGuardService.ts` | WIN-9.3 | todo | — |
| `State/ProjectState.swift` | 748 | `ui/src/state/projectState.ts`, `ui/src/state/remoteLayoutSaver.ts`, `ui/src/state/cachedWorkspacePersistence.ts`, `ui/src/services/fs.ts`, `src-tauri/src/files/text.rs` | WIN-6.0 | ported | — |
| `State/RemoteFolderBrowser+iOS.swift` | 242 | — | — | excluded | `+iOS` file (iPad build only) |
| `State/ScheduleState.swift` | 282 | `ui/src/state/scheduleState.ts` | WIN-8.8 | todo | — |
| `State/SessionListState.swift` | 164 | `ui/src/state/sessions.ts` | WIN-8.16 | todo | — |
| `State/SettingsState.swift` | 111 | `ui/src/state/settingsState.ts` | WIN-8.11 | todo | — |
| `State/SidebarExpansionStore.swift` | 45 | `ui/src/state/sidebarExpansionStore.ts` | WIN-6.5 | ported | — |
| `State/SidebarVisibility.swift` | 38 | `ui/src/state/sidebarVisibilityStore.ts` | WIN-6.5 | ported | — |
| `State/SpawnTargetResolver.swift` | 31 | `ui/src/state/spawnTargetResolver.ts` | WIN-6.6 | ported | — |
| `State/StreamAccumulator.swift` | 193 | `ui/src/state/sessions.ts` | WIN-8.16 | todo | — |
| `State/TriggersState.swift` | 93 | `ui/src/state/triggersState.ts` | WIN-8.9 | todo | — |
| `State/WorkspaceFilesState.swift` | 139 | `ui/src/state/workspaceFilesState.ts` | WIN-8.4 | todo | — |
| `State/WorkspaceRegistry.swift` | 749 | `ui/src/state/workspaceRegistry.ts` | WIN-6.2 | ported | — |
| `Theme/EditorTheme.swift` | 49 | `ui/src/theme/tokens.css` (`--pp-editor-*`) | WIN-5.8 | ported | — |
| `Theme/PurePointTheme.swift` | 63 | `ui/src/theme/tokens.css` (metrics, fonts) | WIN-5.8 | ported | — |
| `Theme/TerminalTheme.swift` | 118 | `ui/src/theme/tokens.css` (`--pp-terminal-*`) | WIN-5.8 | ported | — |
| `Theme/Theme.swift` | 114 | `ui/src/theme/tokens.css` (colors) | WIN-5.8 | ported | — |
| `Utilities/WorktreeNameNormalizer.swift` | 24 | `ui/src/model/worktreeName.ts` | WIN-6.0 | ported | — |
| `Views/Agents/AgentCreationSheet.swift` | 156 | `ui/src/views/agents/AgentCreationSheet.tsx` | WIN-8.7 | todo | — |
| `Views/Agents/AgentsHubView.swift` | 877 | `ui/src/views/agents/AgentsHubView.tsx` | WIN-8.7 | todo | — |
| `Views/Agents/PromptCreationSheet.swift` | 125 | `ui/src/views/agents/PromptCreationSheet.tsx` | WIN-8.7 | todo | — |
| `Views/Agents/SwarmCreationSheet.swift` | 144 | `ui/src/views/agents/SwarmCreationSheet.tsx` | WIN-8.7 | todo | — |
| `Views/Channel/ChannelComposer+iOS.swift` | 330 | — | — | excluded | `+iOS` file (iPad build only) |
| `Views/Channel/ChannelComposer.swift` | 93 | `ui/src/views/channel/ChannelComposer.tsx` | WIN-8.10 | todo | — |
| `Views/Channel/ChannelHoverRow.swift` | 40 | `ui/src/views/channel/ChannelHoverRow.tsx` | WIN-8.10 | todo | — |
| `Views/Channel/ChannelReferenceView.swift` | 37 | `ui/src/views/channel/ChannelReferenceView.tsx` | WIN-8.10 | todo | — |
| `Views/Channel/ProjectChannelView.swift` | 268 | `ui/src/views/channel/ProjectChannelView.tsx` | WIN-8.10 | todo | — |
| `Views/Chat/ChatAreaView.swift` | 95 | — | — | excluded | DEC-11: legacy chat (iPad DEC-9) |
| `Views/Chat/ChatInputView.swift` | 87 | — | — | excluded | DEC-11: legacy chat (iPad DEC-9) |
| `Views/Chat/CodeBlockView.swift` | 9 | — | — | excluded | DEC-11: legacy chat, reached only from `ChatAreaView` |
| `Views/Chat/ContentBlockView.swift` | 67 | — | — | excluded | DEC-11: legacy chat, reached only from `ChatAreaView` |
| `Views/Chat/ConversationSidebarView.swift` | 200 | — | — | excluded | DEC-11: legacy chat (iPad DEC-9) |
| `Views/Chat/MarkdownTextView.swift` | 8 | — | — | excluded | DEC-11: legacy chat, reached only from `ChatAreaView` |
| `Views/Chat/MessageStreamView.swift` | 61 | — | — | excluded | DEC-11: legacy chat, reached only from `ChatAreaView` |
| `Views/Chat/PiPointGuardChatView.swift` | 776 | `ui/src/views/chat/PiPointGuardChatView.tsx` | WIN-9.5 | todo | — |
| `Views/Chat/PiTranscriptScrollCoordinator.swift` | 65 | `ui/src/views/chat/piTranscriptScrollCoordinator.ts` | WIN-9.5 | todo | — |
| `Views/Chat/PointGuardConversationLayout.swift` | 165 | `ui/src/views/chat/PointGuardConversationLayout.tsx` | WIN-8.13 | todo | — |
| `Views/Chat/PointGuardCourtView.swift` | 481 | `ui/src/views/chat/PointGuardCourtView.tsx` | WIN-8.13 | todo | — |
| `Views/Chat/PointGuardPaneControls.swift` | 86 | `ui/src/views/chat/PointGuardPaneControls.tsx` | WIN-8.13 | todo | — |
| `Views/Chat/PointGuardSetupView+iOS.swift` | 152 | — | — | excluded | `+iOS` file (iPad build only) |
| `Views/Chat/PointGuardSetupView+iOSScanner.swift` | 110 | — | — | excluded | `+iOS` file (iPad build only) |
| `Views/Chat/PointGuardSetupView.swift` | 240 | `ui/src/views/chat/PointGuardSetupView.tsx` | WIN-9.5 | todo | — |
| `Views/Chat/PointGuardView.swift` | 84 | `ui/src/views/chat/PointGuardView.tsx` | WIN-8.13 | todo | — |
| `Views/Chat/ToolCallCardView.swift` | 54 | `ui/src/views/chat/ToolCallCardView.tsx` | WIN-9.5 | todo | — |
| `Views/CommandPalette/CommandPalettePanel+iOS.swift` | 195 | — | — | excluded | `+iOS` file (iPad build only) |
| `Views/CommandPalette/CommandPalettePanel+iOSContent.swift` | 809 | — | — | excluded | `+iOS` file (iPad build only) |
| `Views/CommandPalette/CommandPalettePanel.swift` | 887 | `ui/src/views/palette/CommandPalettePanel.tsx` | WIN-7.5 | todo | — |
| `Views/CommandPalette/CommandPaletteRowView+iOS.swift` | 92 | — | — | excluded | `+iOS` file (iPad build only) |
| `Views/CommandPalette/CommandPaletteRowView.swift` | 88 | `ui/src/views/palette/CommandPaletteRowView.tsx` | WIN-7.5 | todo | — |
| `Views/DaemonErrorBanner.swift` | 83 | `ui/src/views/shell/DaemonErrorBanner.tsx`, `ui/src/views/shell/ShellBanners.tsx` | WIN-7.1 | ported | — |
| `Views/Detail/CoreTextBodyView+iOS.swift` | 290 | — | — | excluded | `+iOS` file (iPad build only) |
| `Views/Detail/CoreTextLayout+iOS.swift` | 252 | — | — | excluded | `+iOS` file (iPad build only) |
| `Views/Detail/DetailView.swift` | 104 | `ui/src/views/detail/DetailView.tsx` | WIN-7.1 | ported | — |
| `Views/Detail/DiffCardView.swift` | 202 | `ui/src/views/detail/DiffCardView.tsx` | WIN-8.3 | todo | — |
| `Views/Detail/DiffContentNSView+iOS.swift` | 75 | — | — | excluded | `+iOS` file (iPad build only) |
| `Views/Detail/DiffContentNSView.swift` | 105 | `ui/src/views/detail/DiffContentNSView.tsx` | WIN-8.3 | todo | — |
| `Views/Detail/DiffListView.swift` | 115 | `ui/src/views/detail/DiffListView.tsx` | WIN-8.3 | todo | — |
| `Views/Detail/DiffTextRendering.swift` | 190 | `ui/src/views/detail/DiffTextRendering.tsx` | WIN-8.3 | todo | — |
| `Views/Detail/InlineCodeView+iOS.swift` | 80 | — | — | excluded | `+iOS` file (iPad build only) |
| `Views/Detail/InlineCodeView.swift` | 121 | `ui/src/views/detail/InlineCodeView.tsx` | WIN-8.3 | todo | — |
| `Views/Detail/PRRowView.swift` | 132 | `ui/src/views/detail/PRRowView.tsx` | WIN-8.3 | todo | — |
| `Views/Detail/ProjectDetailView.swift` | 105 | `ui/src/views/detail/ProjectDetailView.tsx` | WIN-8.2 | todo | — |
| `Views/Detail/ReviewChangesView.swift` | 95 | `ui/src/views/detail/ReviewChangesView.tsx` | WIN-8.3 | todo | — |
| `Views/Detail/RootCheckoutDetailView.swift` | 246 | `ui/src/views/detail/RootCheckoutDetailView.tsx` | WIN-8.2 | todo | — |
| `Views/Detail/WorkspaceFilesLayout.swift` | 80 | `ui/src/views/detail/WorkspaceFilesLayout.tsx` | WIN-8.2 | todo | — |
| `Views/Detail/WorkspaceFilesSidebar.swift` | 268 | `ui/src/views/detail/WorkspaceFilesSidebar.tsx` | WIN-8.2 | todo | — |
| `Views/Detail/WorkspaceFilesSplit+iOS.swift` | 125 | — | — | excluded | `+iOS` file (iPad build only) |
| `Views/Detail/WorktreeDetailView.swift` | 285 | `ui/src/views/detail/WorktreeDetailView.tsx` | WIN-8.2 | todo | — |
| `Views/Editor/EditorNSView+iOS.swift` | 138 | — | — | excluded | `+iOS` file (iPad build only) |
| `Views/Editor/EditorNSView.swift` | 134 | `ui/src/views/editor/EditorNSView.tsx` | WIN-8.5 | todo | — |
| `Views/Editor/EditorNoticeBanners.swift` | 54 | `ui/src/views/editor/EditorNoticeBanners.tsx` | WIN-8.5 | todo | — |
| `Views/Editor/EditorTextView+iOS.swift` | 98 | — | — | excluded | `+iOS` file (iPad build only) |
| `Views/Editor/EditorTextView.swift` | 54 | `ui/src/views/editor/EditorTextView.tsx` | WIN-8.5 | todo | — |
| `Views/Editor/FileTreeOutlineView+iOS.swift` | 41 | — | — | excluded | `+iOS` file (iPad build only) |
| `Views/Editor/FileTreeOutlineView.swift` | 48 | `ui/src/views/editor/FileTreeOutlineView.tsx` | WIN-8.4 | todo | — |
| `Views/Editor/FileTreeOutlineViewController+iOS.swift` | 421 | — | — | excluded | `+iOS` file (iPad build only) |
| `Views/Editor/FileTreeOutlineViewController+iOSCells.swift` | 138 | — | — | excluded | `+iOS` file (iPad build only) |
| `Views/Editor/FileTreeOutlineViewController+iOSKeys.swift` | 132 | — | — | excluded | `+iOS` file (iPad build only) |
| `Views/Editor/FileTreeOutlineViewController.swift` | 255 | `ui/src/views/editor/FileTreeOutlineViewController.tsx` | WIN-8.4 | todo | — |
| `Views/Editor/FileTreeSidebarView.swift` | 46 | `ui/src/views/editor/FileTreeSidebarView.tsx` | WIN-8.4 | todo | — |
| `Views/Editor/LineNumberRulerView+iOS.swift` | 144 | — | — | excluded | `+iOS` file (iPad build only) |
| `Views/Editor/LineNumberRulerView.swift` | 135 | `ui/src/views/editor/LineNumberRulerView.tsx` | WIN-8.5 | todo | — |
| `Views/FilePane/FilePaneView.swift` | 171 | `ui/src/views/filePane/FilePaneView.tsx` | WIN-8.5 | todo | — |
| `Views/FilePane/MarkdownPreviewView+iOS.swift` | 98 | — | — | excluded | `+iOS` file (iPad build only) |
| `Views/FilePane/MarkdownPreviewView.swift` | 68 | `ui/src/views/filePane/MarkdownPreviewView.tsx` | WIN-8.6 | todo | — |
| `Views/MockUI/MockWorkspaceComponents.swift` | 101 | — | — | excluded | DEC-11: `Views/MockUI` is unreferenced |
| `Views/PaneGrid/DraggableSplit.swift` | 160 | `ui/src/views/paneGrid/DraggableSplit.tsx` | WIN-7.4 | todo | — |
| `Views/PaneGrid/PaneCellView.swift` | 178 | `ui/src/views/paneGrid/PaneCellView.tsx` | WIN-7.4 | todo | — |
| `Views/PaneGrid/PaneGridView.swift` | 74 | `ui/src/views/paneGrid/PaneGridView.tsx` | WIN-7.4 | todo | — |
| `Views/PaneGrid/PaneSplitNode.swift` | 223 | `ui/src/model/paneSplit.ts` | WIN-6.2 | ported | — |
| `Views/PaneGrid/PaneTabBar.swift` | 439 | `ui/src/views/paneGrid/PaneTabBar.tsx` | WIN-7.4 | todo | — |
| `Views/PaneGrid/PaneTabDragInteraction+iOS.swift` | 172 | — | — | excluded | `+iOS` file (iPad build only) |
| `Views/PaneGrid/WorkspacePersistence.swift` | 562 | `ui/src/model/workspacePersistence.ts`, `ui/src/model/swiftJson.ts`, `ui/src/model/workspaceSelection.ts` | WIN-6.3 | ported | — |
| `Views/PresentedControlDefaults.swift` | 17 | — | — | excluded | DEC-11: unreferenced |
| `Views/Schedule/DayCalendarView.swift` | 28 | `ui/src/views/schedule/DayCalendarView.tsx` | WIN-8.8 | todo | — |
| `Views/Schedule/EventBlockView.swift` | 67 | `ui/src/views/schedule/EventBlockView.tsx` | WIN-8.8 | todo | — |
| `Views/Schedule/EventPillView.swift` | 32 | `ui/src/views/schedule/EventPillView.tsx` | WIN-8.8 | todo | — |
| `Views/Schedule/MonthCalendarView.swift` | 103 | `ui/src/views/schedule/MonthCalendarView.tsx` | WIN-8.8 | todo | — |
| `Views/Schedule/ScheduleEventSheet.swift` | 331 | `ui/src/views/schedule/ScheduleEventSheet.tsx` | WIN-8.17 | todo | — |
| `Views/Schedule/ScheduleHeaderView.swift` | 86 | `ui/src/views/schedule/ScheduleHeaderView.tsx` | WIN-8.8 | todo | — |
| `Views/Schedule/ScheduleListView.swift` | 162 | `ui/src/views/schedule/ScheduleListView.tsx` | WIN-8.8 | todo | — |
| `Views/Schedule/ScheduleView.swift` | 63 | `ui/src/views/schedule/ScheduleView.tsx` | WIN-8.8 | todo | — |
| `Views/Schedule/TimeGridView.swift` | 177 | `ui/src/views/schedule/TimeGridView.tsx` | WIN-8.8 | todo | — |
| `Views/Schedule/WeekCalendarView.swift` | 55 | `ui/src/views/schedule/WeekCalendarView.tsx` | WIN-8.8 | todo | — |
| `Views/Settings/SettingsAboutView.swift` | 74 | `ui/src/views/settings/SettingsAboutView.tsx` | WIN-8.11 | todo | — |
| `Views/Settings/SettingsAgentsView.swift` | 296 | `ui/src/views/settings/SettingsAgentsView.tsx` | WIN-8.11 | todo | — |
| `Views/Settings/SettingsDisplayView.swift` | 49 | `ui/src/views/settings/SettingsDisplayView.tsx` | WIN-8.11 | todo | — |
| `Views/Settings/SettingsGeneralView.swift` | 128 | `ui/src/views/settings/SettingsGeneralView.tsx` | WIN-8.11 | todo | — |
| `Views/Settings/SettingsHotkeysView+iOS.swift` | 106 | — | — | excluded | `+iOS` file (iPad build only) |
| `Views/Settings/SettingsHotkeysView.swift` | 221 | `ui/src/views/settings/SettingsHotkeysView.tsx` | WIN-8.11 | todo | — |
| `Views/Settings/SettingsPointGuardView.swift` | 20 | `ui/src/views/settings/SettingsPointGuardView.tsx` | WIN-9.5 | todo | — |
| `Views/Settings/SettingsSection.swift` | 41 | `ui/src/views/settings/SettingsSection.tsx` | WIN-8.11 | todo | — |
| `Views/Settings/SettingsView.swift` | 138 | `ui/src/views/settings/SettingsView.tsx` | WIN-8.11 | todo | — |
| `Views/Sidebar/AddMachineSheet+iOS.swift` | 390 | — | — | excluded | `+iOS` file (iPad build only) |
| `Views/Sidebar/AddMachineSheet.swift` | 125 | `ui/src/views/sidebar/AddMachineSheet.tsx` | WIN-8.14 | todo | — |
| `Views/Sidebar/RemoteFolderPicker+iOS.swift` | 503 | — | — | excluded | `+iOS` file (iPad build only) |
| `Views/Sidebar/SidebarFooter.swift` | 153 | `ui/src/views/sidebar/SidebarFooter.tsx` | WIN-7.2 | todo | — |
| `Views/Sidebar/SidebarMenuModel.swift` | 263 | `ui/src/model/sidebarMenu.ts` | WIN-6.5 | ported | — |
| `Views/Sidebar/SidebarOutlineModel.swift` | 310 | `ui/src/model/sidebarOutline.ts` | WIN-6.5 | ported | — |
| `Views/Sidebar/SidebarOutlineView+Callbacks.swift` | 124 | `ui/src/views/sidebar/sidebarOutlineViewCallbacks.ts` | WIN-7.2 | todo | — |
| `Views/Sidebar/SidebarOutlineView+iOS.swift` | 37 | — | — | excluded | `+iOS` file (iPad build only) |
| `Views/Sidebar/SidebarOutlineView.swift` | 37 | `ui/src/views/sidebar/SidebarOutlineView.tsx` | WIN-7.2 | todo | — |
| `Views/Sidebar/SidebarOutlineViewController+Actions.swift` | 128 | `ui/src/views/sidebar/sidebarOutlineViewControllerActions.ts` | WIN-7.8 | todo | — |
| `Views/Sidebar/SidebarOutlineViewController+ProjectDrag.swift` | 54 | `ui/src/views/sidebar/sidebarOutlineViewControllerProjectDrag.ts` | WIN-7.8 | todo | — |
| `Views/Sidebar/SidebarOutlineViewController+iOS.swift` | 593 | — | — | excluded | `+iOS` file (iPad build only) |
| `Views/Sidebar/SidebarOutlineViewController+iOSCells.swift` | 344 | — | — | excluded | `+iOS` file (iPad build only) |
| `Views/Sidebar/SidebarOutlineViewController+iOSKeys.swift` | 109 | — | — | excluded | `+iOS` file (iPad build only) |
| `Views/Sidebar/SidebarOutlineViewController+iOSMenus.swift` | 219 | — | — | excluded | `+iOS` file (iPad build only) |
| `Views/Sidebar/SidebarOutlineViewController.swift` | 841 | `ui/src/views/sidebar/SidebarOutlineViewController.tsx` | WIN-7.2 | todo | — |
| `Views/Sidebar/SidebarView.swift` | 108 | `ui/src/views/sidebar/SidebarView.tsx` | WIN-7.2 | todo | — |
| `Views/Terminal/ScrollableTerminal+iOS.swift` | 648 | — | — | excluded | `+iOS` file (iPad build only) |
| `Views/Terminal/ScrollableTerminal+iOSSupport.swift` | 380 | — | — | excluded | `+iOS` file (iPad build only) |
| `Views/Terminal/ScrollableTerminal.swift` | 564 | `ui/src/views/terminal/scrollableTerminal.ts`, `ui/src/views/terminal/terminalTheme.ts`, `ui/src/views/terminal/terminalFontStore.ts` (xterm.js + WebGL with DOM fallback; wheel, SGR mouse, hover and autoscroll are xterm.js built-ins; Shift forces selection; remote-host uploads and file drops are WIN-8.14/WIN-7.4) | WIN-7.3 | ported | — |
| `Views/Terminal/TerminalContainerView+iOS.swift` | 112 | — | — | excluded | `+iOS` file (iPad build only) |
| `Views/Terminal/TerminalContainerView.swift` | 116 | `ui/src/views/terminal/TerminalPane.tsx`, `ui/src/views/terminal/AgentTerminals.tsx` (interim agent list and detail terminals until WIN-7.2/WIN-7.4) | WIN-7.3 | ported | — |
| `Views/Terminal/TerminalPaneView+iOS.swift` | 247 | — | — | excluded | `+iOS` file (iPad build only) |
| `Views/Terminal/TerminalPaneView.swift` | 251 | `ui/src/views/terminal/terminalPaneView.ts` | WIN-7.3 | ported | — |
| `Views/Terminal/TerminalViewCache.swift` | 152 | `ui/src/views/terminal/terminalViewCache.ts`, `ui/src/views/terminal/terminals.ts` (evicts every hidden terminal 30 s after hiding, REQ-WINAPP-061) | WIN-7.3 | ported | — |
| `Views/TitlebarClearance.swift` | 25 | `ui/src/views/shell/TitlebarClearance.tsx` | WIN-7.1 | ported | — |
| `Views/Triggers/TriggerCreationSheet.swift` | 220 | `ui/src/views/triggers/TriggerCreationSheet.tsx` | WIN-8.9 | todo | — |
| `Views/Triggers/TriggerDetailView.swift` | 170 | `ui/src/views/triggers/TriggerDetailView.tsx` | WIN-8.9 | todo | — |
| `Views/WindowChrome+iOS.swift` | 102 | — | — | excluded | `+iOS` file (iPad build only) |
| `Views/WindowChrome.swift` | 42 | `ui/src/views/shell/WindowChrome.tsx`, `src-tauri/src/chrome.rs`, `src-tauri/src/window_state.rs` | WIN-7.1 | ported | — |
| `purepoint_macosApp.swift` | 95 | `ui/src/main.tsx`, `src-tauri/src/main.rs` | WIN-5.1 | ported | — |

## Tests

Test files in `apps/purepoint-macos/purepoint-macosTests/`. Status is `todo`, `ported`, `ios-only` (whole-file `#if os(iOS)`, nothing to port) or `excluded` (tests an excluded source file). A file with suites that port to different leaves has one row per suite. Ported cases keep their Swift case names in the test title (DoD).

| Test file | Suites | Windows test | Leaf | Status | Note |
|---|---|---|---|---|---|
| `ActiveProjectRoutingTests.swift` | all | `ui/src/model/sidebarOutline.test.ts` | WIN-6.5 | ported | — |
| `AddMachineFlowTests.swift` | all | — | — | ios-only | Whole-file `#if os(iOS)` |
| `AgentSignalBadgeTests.swift` | all | — | — | ios-only | Badge-count rules re-expressed as new vitest cases in WIN-7.7 |
| `AgentsHubModelsTests.swift` | all | `ui/src/model/agentsHubModels.test.ts` | WIN-8.7 | todo | — |
| `AgentsHubStateTests.swift` | all | `ui/src/state/agentsHubState.test.ts` | WIN-8.7 | todo | — |
| `AppLifecycleTests.swift` | all | — | — | ios-only | Whole-file `#if os(iOS)` (`AppActivityTests`), though WIN-6.0 lists it; nothing to port |
| `AttachmentUploadTests.swift` | all | `ui/src/services/attachmentUpload.test.ts` | WIN-9.7 | todo | Platform-neutral cases only |
| `ChannelComposerIOSTests.swift` | all | — | — | ios-only | Whole-file `#if os(iOS)` |
| `ChatStateTests.swift` | all | — | — | excluded | Tests `ChatState` (DEC-11 legacy chat) |
| `CitadelSSHLiveTests.swift` | all | — | — | ios-only | Whole-file `#if os(iOS)` |
| `ClaudeConversationIndexTests.swift` | all | `src-tauri/src/conversations/claude.rs` (#[cfg(test)]) | WIN-8.12 | ported | — |
| `ClaudeProcessTests.swift` | all | — | — | excluded | Tests `ClaudeProcess` (DEC-11 legacy chat, no caller; WIN-8.12) |
| `CommandPaletteNamingTests.swift` | all | `ui/src/model/commandPaletteNaming.test.ts` | WIN-6.6 | ported | — |
| `CommandPaletteOpenCoordinatorTests.swift` | all | `ui/src/state/commandPaletteOpenCoordinator.test.ts` | WIN-6.6 | ported | — |
| `CommandPaletteOrderTests.swift` | all | `ui/src/model/commandPaletteOrder.test.ts` | WIN-6.6 | ported | — |
| `CommandPalettePanelIOSTests.swift` | all | — | — | ios-only | Whole-file `#if os(iOS)` |
| `ContentBlockSplitterTests.swift` | all | `ui/src/model/content.test.ts` | WIN-8.16 | todo | — |
| `DaemonClientParseTests.swift` | all | `ui/src/services/daemonConnection.test.ts` | WIN-6.0 | ported | The attach request case is asserted by `src-tauri/src/attach/session_tests.rs`; the three AppKit `ScrollableTerminal` cases are `it.todo` for the terminal view leaf |
| `DaemonTransportTests.swift` | all | `ui/src/services/daemonConnection.test.ts` | WIN-6.0 | ported | Platform-neutral cases only (`sendBlocking` as `send`); Unix-socket and macOS endpoint cases are the named-pipe tests in `src-tauri/src/daemon/client.rs`; stream cases in `src-tauri/src/subscriptions/tests.rs` and `src-tauri/src/attach/session_tests.rs` |
| `DaemonWorkspaceServiceTests.swift` | all | `ui/src/services/workspaceService.test.ts` | WIN-6.0 | ported | — |
| `DiffLayoutTests.swift` | all | `ui/src/views/detail/diffLayout.test.ts` | WIN-8.3 | todo | — |
| `DiffRenderingIOSTests.swift` | all | — | — | ios-only | Whole-file `#if os(iOS)` |
| `DraggableSplitTests.swift` | all | `ui/src/model/paneSplit.test.ts` | WIN-6.2 | ported | — |
| `EditorIOSTests.swift` | all | — | — | ios-only | Whole-file `#if os(iOS)` |
| `FilePaneTests.swift` | all | `ui/src/views/filePane/FilePaneView.test.tsx` | WIN-8.5 | todo | Relevant cases only |
| `FileTreeOutlineIOSTests.swift` | all | — | — | ios-only | Whole-file `#if os(iOS)` |
| `FileTreeStateTests.swift` | all | `ui/src/state/fileTreeState.test.ts` | WIN-8.4 | todo | — |
| `GitReviewTests.swift` | all | `src-tauri/src/git/tests/git_review.rs` | WIN-8.1 | ported | DiffState-only assertions go with WIN-6.0; WorktreeWatcher checks with WIN-5.6 |
| `HexEncodingTests.swift` | all | `ui/src/services/daemonConnection.test.ts` | WIN-6.0 | ported | — |
| `HotkeyKeyCommandTests.swift` | all | — | — | ios-only | Whole-file `#if os(iOS)` |
| `HotkeyRecorderTests.swift` | all | — | — | ios-only | Windows recorder gets new tests in WIN-8.11 |
| `IPadWindowingTests.swift` | all | — | — | ios-only | Whole-file `#if os(iOS)` |
| `KeyBindingStateTests.swift` | all | `ui/src/state/keyBindingStore.test.ts` | WIN-6.7 | ported | — |
| `LocalFilePresenterIOSTests.swift` | all | — | — | ios-only | Whole-file `#if os(iOS)` |
| `MachinesTests.swift` | all | `ui/src/model/machine.test.ts`, `src-tauri/src/daemon/machine.rs` (#[cfg(test)]) | WIN-8.14 | todo | — |
| `MarkdownHTMLRendererTests.swift` | all | `ui/src/services/markdownHtml.test.ts` | WIN-8.6 | todo | — |
| `PaneTabDragIOSTests.swift` | all | — | — | ios-only | Whole-file `#if os(iOS)` |
| `PaneTabStackTests.swift` | all | `ui/src/state/workspaceRegistry.tabs.test.ts` | WIN-6.2 | ported | — |
| `PointGuardAttachmentIOSTests.swift` | all | — | — | ios-only | Whole-file `#if os(iOS)` |
| `PointGuardPairingTests.swift` | all | — | — | ios-only | Whole-file `#if os(iOS)` |
| `PointGuardPlaysTests.swift` | all | `ui/src/model/pointGuardPlays.test.ts` | WIN-8.13 | todo | — |
| `PointGuardRoutingTests.swift` | all | `ui/src/state/pointGuardRouting.test.ts` | WIN-8.13 | todo | — |
| `PointGuardWorkspaceTests.swift` | all | `ui/src/state/pointGuardWorkspace.test.ts` | WIN-8.13 | todo | — |
| `ProjectOrderTests.swift` | all | `ui/src/model/projectOrder.test.ts` | WIN-6.5 | ported | — |
| `RemoteFolderPickerTests.swift` | `FsBrowseRequestTests` | `ui/src/state/machineState.fsBrowse.test.ts` | WIN-8.14 | todo | Only `FsBrowseRequestTests` is platform-neutral; the rest is `#if os(iOS)` |
| `SSHIdentityTests.swift` | all | — | — | ios-only | Whole-file `#if os(iOS)` |
| `SSHMachineRegistryTests.swift` | all | — | — | ios-only | Whole-file `#if os(iOS)` |
| `SSHTransportTests.swift` | all | — | — | ios-only | Whole-file `#if os(iOS)` |
| `ScrollableTerminalIOSTests.swift` | all | — | — | ios-only | Whole-file `#if os(iOS)` |
| `SessionListStateTests.swift` | all | `ui/src/state/sessions.test.ts` | WIN-8.16 | todo | — |
| `SidebarExpansionStoreTests.swift` | all | `ui/src/state/sidebarExpansionStore.test.ts` | WIN-6.5 | ported | — |
| `SidebarMenuModelTests.swift` | all | `ui/src/model/sidebarMenu.test.ts` | WIN-6.5 | ported | — |
| `SidebarOutlineModelTests.swift` | all | `ui/src/model/sidebarOutline.test.ts` | WIN-6.5 | ported | — |
| `SidebarOutlineViewControllerIOSMenuTests.swift` | all | — | — | ios-only | Whole-file `#if os(iOS)` |
| `SidebarOutlineViewControllerIOSTests.swift` | all | — | — | ios-only | Whole-file `#if os(iOS)` |
| `SidebarOutlineViewControllerTests.swift` | all | `ui/src/views/sidebar/SidebarOutline.test.tsx` | WIN-7.2 | todo | Platform-neutral cases only |
| `SidebarVisibilityTests.swift` | all | `ui/src/state/sidebarVisibilityStore.test.ts` | WIN-6.5 | ported | — |
| `SpawnTargetResolverTests.swift` | all | `ui/src/state/spawnTargetResolver.test.ts` | WIN-6.6 | ported | — |
| `StreamAccumulatorTests.swift` | all | `ui/src/state/streamAccumulator.test.ts` | WIN-8.16 | todo | — |
| `StreamEventTests.swift` | all | `src-tauri/src/conversations/stream_event.rs` (#[cfg(test)]) | WIN-8.12 | ported | — |
| `TabCloseConfirmationIOSTests.swift` | all | — | — | ios-only | Whole-file `#if os(iOS)` |
| `TerminalOutputFilterTests.swift` | all | `ui/src/views/terminal/outputFilter.test.ts` | WIN-7.3 | ported | No filter ported: xterm.js implements DEC 2026 synchronized output and clamps cursor-up itself; the test runs the Swift inputs through `@xterm/headless` |
| `TranscriptParserTests.swift` | all | `src-tauri/src/conversations/transcript.rs` (#[cfg(test)]) | WIN-8.12 | ported | — |
| `WorkingTreeDiffTests.swift` | all | `src-tauri/src/git/tests/working_tree_diff.rs` | WIN-8.1 | ported | — |
| `WorkspaceFilesStateTests.swift` | all | `ui/src/state/workspaceFilesState.test.ts` | WIN-8.4 | todo | — |
| `WorkspaceFixtureTests.swift` | all | `ui/src/model/workspacePersistence.test.ts` | WIN-6.3 | ported | Swift side of the shared `docs/reference/fixtures/workspaces/` golden files; run in Xcode by WIN-11.2 |
| `WorkspaceModelTests.swift` | all | `ui/src/model/workspaceModel.test.ts` | WIN-6.1 | ported | — |
| `WorkspaceReconcilerTests.swift` | all | `ui/src/model/workspace.test.ts`, `ui/src/model/workspace.property.test.ts` | WIN-6.1 | ported | The `WorkspacePersistence` cases run against the WIN-6.3 port over an in-memory file |
| `WorkspaceRegistryTests.swift` | all | `ui/src/state/workspaceRegistry.test.ts` | WIN-6.2 | ported | — |
| `purepoint_macosTests.swift` | `GlobalAgentSettingsTests` | `ui/src/state/agentConfigState.test.ts` | WIN-8.7 | todo | — |
| `purepoint_macosTests.swift` | `PaneSplitNodeTests` | `ui/src/model/paneSplit.test.ts` | WIN-6.2 | ported | — |
| `purepoint_macosTests.swift` | `GridLayoutPersistenceTests` | `ui/src/model/workspacePersistence.test.ts` | WIN-6.3 | todo | — |

## SF Symbols

Every SF Symbol name in the Swift sources, mapped to a [Lucide](https://lucide.dev/icons/) icon (DEC-13). WIN-5.8 builds `ui/src/theme/icons.ts` from this table. Names are lucide-react 1.x canonical names, not deprecated aliases (`circle-exclamation-point`, not `circle-alert`); `ui/src/theme/icons.test.tsx` fails on a name lucide-react does not export. `.fill` variants map to the same Lucide icon, drawn with `fill="currentColor"` where the Mac shows a filled glyph. The checker finds symbols in `systemName:`, `systemImage:` and `systemSymbolName:` arguments, `icon:`/`symbol:` labels, `icon`/`symbol` properties and the symbol helper functions (`makeSymbolIcon`, `actionIcon` and similar).

| SF Symbol | Lucide icon | Used in |
|---|---|---|
| `arrow.clockwise` | `rotate-cw` | `Views/Chat/PiPointGuardChatView.swift`, `Views/Detail/RootCheckoutDetailView.swift`, `Views/Detail/WorktreeDetailView.swift` |
| `arrow.counterclockwise` | `rotate-ccw` | `Views/Settings/SettingsHotkeysView.swift` |
| `arrow.down` | `arrow-down` | `Views/Channel/ProjectChannelView.swift`, `Views/Chat/PiPointGuardChatView.swift` |
| `arrow.right.arrow.left` | `arrow-right-left` | `Models/EditorTab.swift` |
| `arrow.triangle.branch` | `git-branch` | `Models/AgentVariant.swift`, `Views/Chat/ConversationSidebarView.swift`, `Views/Detail/DetailView.swift` (+5) |
| `arrow.triangle.pull` | `git-pull-request` | `Views/Channel/ProjectChannelView.swift` |
| `arrow.turn.up.right` | `corner-up-right` | `Views/Sidebar/RemoteFolderPicker+iOS.swift` |
| `arrow.up` | `arrow-up` | `Views/Channel/ProjectChannelView.swift`, `Views/Chat/ChatInputView.swift`, `Views/Chat/PiPointGuardChatView.swift` |
| `arrow.up.circle` | `circle-arrow-up` | `Models/TriggerItem.swift` |
| `arrow.up.right.square` | `square-arrow-up-right-exit` | `Views/Detail/ReviewChangesView.swift` |
| `bolt` | `zap` | `Views/Triggers/TriggerDetailView.swift` |
| `bubble.left` | `message-circle` | `Views/Channel/ChannelHoverRow.swift` |
| `bubble.left.and.bubble.right` | `messages-square` | `Views/Channel/ProjectChannelView.swift`, `Views/Chat/PointGuardConversationLayout.swift`, `Views/Detail/DetailView.swift` (+2) |
| `bubble.left.and.bubble.right.fill` | `messages-square` | `Models/SidebarItem.swift`, `Views/Chat/ChatAreaView.swift` |
| `c.square` | `square-code` | `Models/EditorTab.swift` |
| `calendar` | `calendar` | `Models/SidebarItem.swift` |
| `checkmark` | `check` | `Platform/MacControlStyles+iOS.swift`, `State/AppState+LibraryScope+iOS.swift`, `Views/Sidebar/RemoteFolderPicker+iOS.swift` |
| `checkmark.circle` | `circle-check` | `Models/TriggerItem.swift`, `Views/Chat/PointGuardSetupView+iOS.swift`, `Views/Chat/PointGuardSetupView.swift` (+3) |
| `checkmark.circle.fill` | `circle-check` | `Views/Chat/PointGuardSetupView.swift`, `Views/Detail/PRRowView.swift` |
| `chevron.down` | `chevron-down` | `Views/Detail/DiffCardView.swift`, `Views/Detail/WorkspaceFilesSidebar.swift`, `Views/Settings/SettingsGeneralView.swift` |
| `chevron.left` | `chevron-left` | `Views/Detail/ReviewChangesView.swift`, `Views/Schedule/ScheduleHeaderView.swift`, `Views/Sidebar/RemoteFolderPicker+iOS.swift` |
| `chevron.left.forwardslash.chevron.right` | `code` | `Models/AgentVariant.swift`, `Models/EditorTab.swift`, `Views/Channel/ProjectChannelView.swift` |
| `chevron.right` | `chevron-right` | `Views/Detail/DiffCardView.swift`, `Views/Detail/ProjectDetailView.swift`, `Views/Detail/RootCheckoutDetailView.swift` (+6) |
| `chevron.up` | `chevron-up` | `Views/Settings/SettingsGeneralView.swift` |
| `chevron.up.chevron.down` | `chevrons-up-down` | `State/AppState+LibraryScope+iOS.swift`, `Views/Sidebar/RemoteFolderPicker+iOS.swift` |
| `circle` | `circle` | `Views/Chat/PointGuardSetupView+iOS.swift`, `Views/Chat/PointGuardSetupView.swift`, `Views/Detail/PRRowView.swift` |
| `circle.fill` | `circle` | `Models/AgentVariant.swift` |
| `clock` | `clock` | `Views/Schedule/ScheduleEventSheet.swift` |
| `command` | `command` | `Views/Agents/AgentsHubView.swift` |
| `cpu` | `cpu` | `Models/CommandPaletteItem.swift`, `Models/ScheduleEvent.swift`, `Models/SidebarItem.swift` (+3) |
| `cup.and.saucer` | `coffee` | `Models/EditorTab.swift` |
| `curlybraces` | `braces` | `Models/EditorTab.swift` |
| `desktopcomputer` | `monitor` | `State/AppState+LibraryScope+iOS.swift`, `Views/Chat/PointGuardSetupView+iOS.swift`, `Views/Settings/SettingsAgentsView.swift` (+5) |
| `desktopcomputer.trianglebadge.exclamationmark` | `monitor-x` | `Views/Sidebar/RemoteFolderPicker+iOS.swift` |
| `diamond` | `diamond` | `Models/AgentVariant.swift`, `Models/EditorTab.swift` |
| `doc` | `file` | `Models/EditorTab.swift`, `Views/Editor/FileTreeOutlineViewController+iOSCells.swift`, `Views/Editor/FileTreeOutlineViewController.swift` |
| `doc.badge.gearshape` | `file-cog` | `Models/EditorTab.swift` |
| `doc.badge.minus` | `file-minus` | `Views/Detail/DiffCardView.swift` |
| `doc.badge.plus` | `file-plus` | `Views/Detail/DiffCardView.swift`, `Views/Detail/RootCheckoutDetailView.swift`, `Views/Detail/WorktreeDetailView.swift` |
| `doc.fill` | `file` | `Views/Detail/RootCheckoutDetailView.swift`, `Views/Detail/WorktreeDetailView.swift`, `Views/FilePane/FilePaneView.swift` |
| `doc.text` | `file-text` | `Models/EditorTab.swift`, `Views/Chat/PiPointGuardChatView.swift`, `Views/Detail/DiffCardView.swift` (+5) |
| `ellipsis` | `ellipsis` | `Views/Channel/ChannelHoverRow.swift`, `Views/Channel/ProjectChannelView.swift`, `Views/Chat/PointGuardPaneControls.swift` |
| `exclamationmark.circle` | `circle-exclamation-point` | `Views/Channel/ProjectChannelView.swift` |
| `exclamationmark.triangle` | `triangle-exclamation-point` | `Views/Chat/PointGuardView.swift`, `Views/Detail/DiffListView.swift`, `Views/Detail/ReviewChangesView.swift` (+2) |
| `exclamationmark.triangle.fill` | `triangle-exclamation-point` | `Views/DaemonErrorBanner.swift`, `Views/Detail/RootCheckoutDetailView.swift`, `Views/Detail/WorktreeDetailView.swift` (+5) |
| `eye` | `eye` | `Views/Sidebar/RemoteFolderPicker+iOS.swift` |
| `eye.slash` | `eye-off` | `Views/Sidebar/RemoteFolderPicker+iOS.swift` |
| `folder` | `folder` | `Models/CommandPaletteItem.swift`, `Views/Chat/PiPointGuardChatView.swift`, `Views/Detail/ProjectDetailView.swift` (+4) |
| `folder.badge.questionmark` | `folder-search` | `Views/Settings/SettingsAgentsView.swift` |
| `folder.fill` | `folder` | `Views/Detail/DetailView.swift`, `Views/Detail/RootCheckoutDetailView.swift`, `Views/Editor/FileTreeOutlineViewController+iOSCells.swift` (+4) |
| `function` | `square-function` | `Models/EditorTab.swift` |
| `gear` | `settings` | `Views/Settings/SettingsSection.swift`, `Views/Sidebar/SidebarFooter.swift` |
| `gearshape` | `settings` | `Views/Chat/PiPointGuardChatView.swift` |
| `gearshape.2` | `cog` | `Models/EditorTab.swift` |
| `globe` | `globe` | `Models/EditorTab.swift`, `Views/Agents/AgentsHubView.swift` |
| `hand.thumbsup` | `thumbs-up` | `Views/Channel/ChannelHoverRow.swift` |
| `info.circle` | `info` | `Views/Settings/SettingsSection.swift`, `Views/Sidebar/AddMachineSheet+iOS.swift` |
| `ipad` | `tablet` | `Views/Chat/PointGuardSetupView.swift` |
| `iphone` | `smartphone` | `Views/Chat/PointGuardSetupView.swift` |
| `keyboard` | `keyboard` | `Views/Settings/SettingsSection.swift` |
| `lock.fill` | `lock` | `Views/Editor/EditorNoticeBanners.swift` |
| `lock.shield` | `shield-check` | `Views/Sidebar/AddMachineSheet+iOS.swift` |
| `magnifyingglass` | `search` | `Views/Channel/ProjectChannelView.swift`, `Views/PaneGrid/PaneTabBar.swift` |
| `minus.circle` | `circle-minus` | `Views/Triggers/TriggerCreationSheet.swift` |
| `minus.circle.fill` | `circle-minus` | `Views/Agents/SwarmCreationSheet.swift` |
| `moon` | `moon` | `Models/EditorTab.swift` |
| `moon.zzz` | `moon-star` | `Models/TriggerItem.swift` |
| `number` | `hash` | `Models/EditorTab.swift` |
| `paintbrush` | `paintbrush` | `Models/EditorTab.swift`, `Views/Settings/SettingsSection.swift` |
| `person` | `user` | `Views/Channel/ProjectChannelView.swift`, `Views/Detail/PRRowView.swift` |
| `person.2` | `users` | `Views/Channel/ProjectChannelView.swift` |
| `person.3` | `users-round` | `Models/CommandPaletteItem.swift` |
| `person.3.fill` | `users-round` | `Models/ScheduleEvent.swift` |
| `photo` | `image` | `Views/Chat/PiPointGuardChatView.swift` |
| `plus` | `plus` | `Views/Agents/AgentsHubView.swift`, `Views/Chat/PiPointGuardChatView.swift`, `Views/PaneGrid/PaneTabBar.swift` (+3) |
| `plus.bubble` | `message-circle-plus` | `Views/Chat/ConversationSidebarView.swift` |
| `plus.circle` | `circle-plus` | `Views/Agents/SwarmCreationSheet.swift`, `Views/Sidebar/SidebarOutlineViewController+iOSCells.swift`, `Views/Sidebar/SidebarOutlineViewController.swift` (+1) |
| `point.3.connected.trianglepath.dotted` | `git-commit-horizontal` | `Views/Channel/ProjectChannelView.swift`, `Views/Detail/ReviewChangesView.swift` |
| `rectangle.dashed` | `square-dashed` | `Views/PaneGrid/PaneCellView.swift` |
| `rectangle.split.1x2` | `rows-2` | `Views/PaneGrid/PaneTabBar.swift` |
| `rectangle.split.2x1` | `columns-2` | `Views/PaneGrid/PaneTabBar.swift` |
| `rectangle.split.2x2` | `grid-2x2` | `Views/Detail/DetailView.swift`, `Views/Sidebar/SidebarOutlineViewController+iOS.swift`, `Views/Sidebar/SidebarOutlineViewController.swift` |
| `s.square` | `square-code` | `Models/EditorTab.swift` |
| `safari` | `compass` | `Models/PointGuardPlays.swift` |
| `server.rack` | `server` | `Models/EditorTab.swift` |
| `shield.checkered` | `shield` | `Views/Triggers/TriggerDetailView.swift` |
| `sidebar.left` | `panel-left` | `ContentView.swift`, `Views/Chat/ConversationSidebarView.swift`, `Views/Chat/PointGuardConversationLayout.swift` (+5) |
| `sidebar.right` | `panel-right` | `Views/Detail/DiffListView.swift`, `Views/Detail/WorkspaceFilesLayout.swift` |
| `sparkle` | `sparkle` | `Views/PaneGrid/PaneTabBar.swift` |
| `square.and.pencil` | `square-pen` | `Views/Chat/PiPointGuardChatView.swift` |
| `square.grid.2x2` | `layout-grid` | `Models/PointGuardPlays.swift` |
| `square.stack` | `square-stack` | `Views/Sidebar/SidebarOutlineViewController+iOS.swift`, `Views/Sidebar/SidebarOutlineViewController.swift` |
| `square.stack.3d.up` | `layers` | `Models/PointGuardPlays.swift` |
| `stop.fill` | `square` | `Views/Chat/ChatInputView.swift`, `Views/Chat/PiPointGuardChatView.swift` |
| `swift` | `bird` | `Models/EditorTab.swift` |
| `terminal` | `terminal` | `Models/AgentVariant.swift`, `Models/EditorTab.swift`, `Views/DaemonErrorBanner.swift` (+2) |
| `text.alignleft` | `text-align-start` | `Views/Agents/AgentsHubView.swift`, `Views/Chat/PiPointGuardChatView.swift` |
| `text.bubble` | `message-square-text` | `Models/PointGuardPlays.swift`, `Models/ScheduleEvent.swift`, `Views/Chat/ConversationSidebarView.swift` |
| `text.cursor` | `text-cursor` | `Views/Triggers/TriggerDetailView.swift` |
| `waveform.path.ecg` | `activity` | `Models/PointGuardPlays.swift`, `Views/Chat/ContentBlockView.swift` |
| `xmark` | `x` | `Views/Channel/ProjectChannelView.swift`, `Views/Chat/PiPointGuardChatView.swift`, `Views/DaemonErrorBanner.swift` (+2) |
| `xmark.circle` | `circle-x` | `Views/PaneGrid/PaneTabBar.swift` |
| `xmark.circle.fill` | `circle-x` | `Views/Detail/PRRowView.swift`, `Views/Detail/RootCheckoutDetailView.swift`, `Views/Detail/WorktreeDetailView.swift` |
| `xmark.rectangle` | `square-x` | `Views/PaneGrid/PaneTabBar.swift` |
