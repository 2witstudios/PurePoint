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
            CommandGroup(after: .newItem) {
                Button("Open Project...") {
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
                UserDefaults.standard.set(path, forKey: "PurePointLastProject")
                appState.openProject(path)
                return
            }
        }

        // Try last opened project from UserDefaults
        if let lastProject = UserDefaults.standard.string(forKey: "PurePointLastProject"),
           FileManager.default.fileExists(atPath: lastProject) {
            appState.openProject(lastProject)
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
            gridState.projectRoot = appState.projectRoot
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
            let path = url.path
            UserDefaults.standard.set(path, forKey: "PurePointLastProject")
            appState.openProject(path)
        }
    }
}
