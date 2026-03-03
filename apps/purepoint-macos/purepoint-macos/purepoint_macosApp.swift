import SwiftUI

@main
struct purepoint_macosApp: App {
    @State private var appState = AppState()
    @State private var viewCache = TerminalViewCache()
    @State private var gridState = GridState()

    var body: some Scene {
        WindowGroup {
            ContentView()
                .environment(appState)
                .environment(viewCache)
                .environment(gridState)
                .frame(
                    minWidth: PurePointTheme.windowMinWidth,
                    minHeight: PurePointTheme.windowMinHeight
                )
                .onAppear {
                    appState.gridState = gridState
                    openProjectFromArguments()
                }
                .onReceive(NotificationCenter.default.publisher(for: NSApplication.willTerminateNotification)) { _ in
                    appState.shutdown()
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
                .keyboardShortcut("n")
                .disabled(appState.projects.isEmpty)

                Button("Open Project\u{2026}") {
                    openProjectWithPicker()
                }
                .keyboardShortcut("o")
            }

            CommandMenu("Panes") {
                Button("Split Below") {
                    splitOrEnterGrid(axis: .horizontal)
                }
                .keyboardShortcut("d")
                .disabled(!canSplitBelow)

                Button("Split Right") {
                    splitOrEnterGrid(axis: .vertical)
                }
                .keyboardShortcut("d", modifiers: [.command, .shift])
                .disabled(!canSplitRight)

                Button("Close Pane") {
                    gridState.closeFocused()
                }
                .keyboardShortcut("w", modifiers: [.command, .shift])
                .disabled(!gridState.isActive || gridState.leafCount <= 1)

                Divider()

                Button("Focus Up") {
                    gridState.moveFocus(direction: .up)
                }
                .keyboardShortcut(.upArrow, modifiers: [.command, .option])
                .disabled(!gridState.isActive)

                Button("Focus Down") {
                    gridState.moveFocus(direction: .down)
                }
                .keyboardShortcut(.downArrow, modifiers: [.command, .option])
                .disabled(!gridState.isActive)

                Button("Focus Left") {
                    gridState.moveFocus(direction: .left)
                }
                .keyboardShortcut(.leftArrow, modifiers: [.command, .option])
                .disabled(!gridState.isActive)

                Button("Focus Right") {
                    gridState.moveFocus(direction: .right)
                }
                .keyboardShortcut(.rightArrow, modifiers: [.command, .option])
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
        CommandPalettePanel.show(relativeTo: NSApp.keyWindow) { variant, prompt in
            project.createAgent(variant: variant, prompt: prompt, selection: nil)
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
