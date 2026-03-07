import Sparkle
import SwiftUI

@main
struct purepoint_macosApp: App {
    @State private var appState = AppState()
    @State private var settingsState = SettingsState()
    @State private var viewCache = TerminalViewCache()
    @State private var gridState = GridState()
    @State private var keyBindingState = KeyBindingState()
    @State private var hotkeyMonitor = HotkeyMonitor()
    @StateObject private var updaterViewModel = CheckForUpdatesViewModel()

    var body: some Scene {
        WindowGroup {
            ContentView()
                .environment(appState)
                .environment(settingsState)
                .environment(viewCache)
                .environment(gridState)
                .environment(keyBindingState)
                .environmentObject(updaterViewModel)
                .preferredColorScheme(settingsState.appearance.colorScheme)
                .frame(
                    minWidth: PurePointTheme.windowMinWidth,
                    minHeight: PurePointTheme.windowMinHeight
                )
                .onAppear {
                    appState.gridState = gridState
                    gridState.onCloseAgent = { agentId in
                        appState.projectState(forRoot: gridState.projectRoot ?? "")?.removeAndKillAgent(agentId)
                    }
                    hotkeyMonitor.keyBindingState = keyBindingState
                    hotkeyMonitor.start()
                    CLIInstaller.installIfNeeded()
                    openProjectFromArguments()
                }
                .onReceive(NotificationCenter.default.publisher(for: NSApplication.willTerminateNotification)) { _ in
                    appState.shutdownWithSuspend()
                }
        }
        .defaultSize(
            width: PurePointTheme.windowDefaultWidth,
            height: PurePointTheme.windowDefaultHeight
        )
        .windowToolbarStyle(.unified(showsTitle: true))
        .commands {
            CommandGroup(replacing: .newItem) {
                Button("New Agent\u{2026}") {
                    showNewAgentPalette()
                }
                .keyboardShortcut(
                    keyBindingState.keyEquivalent(for: .newAgent),
                    modifiers: keyBindingState.eventModifiers(for: .newAgent)
                )
                .disabled(appState.projects.isEmpty)

                Button("Open Project\u{2026}") {
                    openProjectWithPicker()
                }
                .keyboardShortcut(
                    keyBindingState.keyEquivalent(for: .openProject),
                    modifiers: keyBindingState.eventModifiers(for: .openProject)
                )
            }

            CommandGroup(replacing: .appSettings) {
                Button("Settings\u{2026}") {
                    appState.showSettings = true
                }
                .keyboardShortcut(
                    keyBindingState.keyEquivalent(for: .settings),
                    modifiers: keyBindingState.eventModifiers(for: .settings)
                )

                Divider()

                Button("Check for Updates\u{2026}") {
                    updaterViewModel.checkForUpdates()
                }
                .disabled(!updaterViewModel.canCheckForUpdates)
            }

            CommandMenu("Navigation") {
                Button("Focus Sidebar") {
                    postHotkeyAction(.focusSidebar)
                }
                .keyboardShortcut(
                    keyBindingState.keyEquivalent(for: .focusSidebar),
                    modifiers: keyBindingState.eventModifiers(for: .focusSidebar)
                )

                Button("Focus Content") {
                    postHotkeyAction(.focusContent)
                }
                .keyboardShortcut(
                    keyBindingState.keyEquivalent(for: .focusContent),
                    modifiers: keyBindingState.eventModifiers(for: .focusContent)
                )

                Button("Toggle Sidebar") {
                    postHotkeyAction(.toggleSidebar)
                }
                .keyboardShortcut(
                    keyBindingState.keyEquivalent(for: .toggleSidebar),
                    modifiers: keyBindingState.eventModifiers(for: .toggleSidebar)
                )

                Divider()

                Button("Dashboard") {
                    postHotkeyAction(.navDashboard)
                }
                .keyboardShortcut(
                    keyBindingState.keyEquivalent(for: .navDashboard),
                    modifiers: keyBindingState.eventModifiers(for: .navDashboard)
                )

                Button("Agents") {
                    postHotkeyAction(.navAgents)
                }
                .keyboardShortcut(
                    keyBindingState.keyEquivalent(for: .navAgents),
                    modifiers: keyBindingState.eventModifiers(for: .navAgents)
                )

                Button("Schedule") {
                    postHotkeyAction(.navSchedule)
                }
                .keyboardShortcut(
                    keyBindingState.keyEquivalent(for: .navSchedule),
                    modifiers: keyBindingState.eventModifiers(for: .navSchedule)
                )

                Divider()

                Button("Close Agent") {
                    postHotkeyAction(.closeAgent)
                }
                .keyboardShortcut(
                    keyBindingState.keyEquivalent(for: .closeAgent),
                    modifiers: keyBindingState.eventModifiers(for: .closeAgent)
                )
                .disabled(appState.selectedAgentId == nil && !gridState.isActive)
            }

            CommandMenu("Panes") {
                Button("Split Below") {
                    splitOrEnterGrid(axis: .horizontal)
                }
                .keyboardShortcut(
                    keyBindingState.keyEquivalent(for: .splitBelow),
                    modifiers: keyBindingState.eventModifiers(for: .splitBelow)
                )
                .disabled(!canSplitBelow)

                Button("Split Right") {
                    splitOrEnterGrid(axis: .vertical)
                }
                .keyboardShortcut(
                    keyBindingState.keyEquivalent(for: .splitRight),
                    modifiers: keyBindingState.eventModifiers(for: .splitRight)
                )
                .disabled(!canSplitRight)

                Button("Close Pane") {
                    gridState.closeFocused()
                }
                .keyboardShortcut(
                    keyBindingState.keyEquivalent(for: .closePane),
                    modifiers: keyBindingState.eventModifiers(for: .closePane)
                )
                .disabled(!gridState.isActive || gridState.leafCount <= 1)

                Divider()

                Button("Focus Up") {
                    gridState.moveFocus(direction: .up)
                }
                .keyboardShortcut(
                    keyBindingState.keyEquivalent(for: .focusUp),
                    modifiers: keyBindingState.eventModifiers(for: .focusUp)
                )
                .disabled(!gridState.isActive)

                Button("Focus Down") {
                    gridState.moveFocus(direction: .down)
                }
                .keyboardShortcut(
                    keyBindingState.keyEquivalent(for: .focusDown),
                    modifiers: keyBindingState.eventModifiers(for: .focusDown)
                )
                .disabled(!gridState.isActive)

                Button("Focus Left") {
                    gridState.moveFocus(direction: .left)
                }
                .keyboardShortcut(
                    keyBindingState.keyEquivalent(for: .focusLeft),
                    modifiers: keyBindingState.eventModifiers(for: .focusLeft)
                )
                .disabled(!gridState.isActive)

                Button("Focus Right") {
                    gridState.moveFocus(direction: .right)
                }
                .keyboardShortcut(
                    keyBindingState.keyEquivalent(for: .focusRight),
                    modifiers: keyBindingState.eventModifiers(for: .focusRight)
                )
                .disabled(!gridState.isActive)
            }
        }
    }

    private func openProjectFromArguments() {
        // Check command-line arguments for --project-root
        let args = ProcessInfo.processInfo.arguments
        for (i, arg) in args.enumerated() {
            if arg == "--project-root", i + 1 < args.count {
                let path = args[i + 1]
                appState.openProject(path)
                return
            }
        }

        // Restore saved projects from UserDefaults
        appState.restoreProjects()

        // Restore selected agent after projects have loaded
        Task { @MainActor in
            try? await Task.sleep(nanoseconds: 500_000_000)
            appState.restoreSelectedAgent()
        }
    }

    // MARK: - Command Palette

    private func showNewAgentPalette() {
        let project: ProjectState?
        if let root = appState.activeProjectRoot {
            project = appState.projectState(forRoot: root)
        } else {
            project = appState.projects.first
        }
        guard let project else { return }

        let hub = appState.agentsHubState
        let items = CommandPaletteItem.buildItems(
            builtInVariants: AgentVariant.variantsWithWorktree,
            agents: hub.agents,
            swarms: hub.swarms
        )
        Task { await hub.loadAll(projectRoot: project.projectRoot) }

        let sel = appState.activeSidebarSelection
        CommandPalettePanel.show(relativeTo: NSApp.keyWindow, items: items) { result in
            project.handlePaletteResult(result, selection: sel, hub: hub)
        }
    }

    // MARK: - Split Helpers

    /// Can split below: either grid is active and can split, or we're in single-pane with an agent.
    private var canSplitBelow: Bool {
        if gridState.isActive { return gridState.canSplit(axis: .horizontal) }
        return appState.selectedAgentId != nil
    }

    /// Can split right: either grid is active and can split, or we're in single-pane with an agent.
    private var canSplitRight: Bool {
        if gridState.isActive { return gridState.canSplit(axis: .vertical) }
        return appState.selectedAgentId != nil
    }

    /// Split the focused pane (if grid active) or enter grid mode from single-pane.
    private func splitOrEnterGrid(axis: PaneSplitNode.Axis) {
        if gridState.isActive {
            gridState.splitFocused(axis: axis)
        } else if let agentId = appState.selectedAgentId {
            if let project = appState.projectState(forAgentId: agentId) {
                gridState.projectRoot = project.projectRoot
            }
            gridState.enterGridMode(agentId: agentId, axis: axis)
        }
        gridState.pendingPaletteLeafId = gridState.focusedLeafId
    }

    private func postHotkeyAction(_ action: HotkeyAction) {
        NotificationCenter.default.post(
            name: .hotkeyAction,
            object: nil,
            userInfo: ["action": action]
        )
    }

    private func openProjectWithPicker() {
        let panel = NSOpenPanel()
        panel.canChooseDirectories = true
        panel.canChooseFiles = false
        panel.allowsMultipleSelection = false
        panel.message = "Choose a project directory"

        if panel.runModal() == .OK, let url = panel.url {
            appState.openProject(url.path)
        }
    }
}
