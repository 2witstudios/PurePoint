import SwiftUI

struct ContentView: View {
    @Environment(AppState.self) private var appState
    @Environment(GridState.self) private var gridState
    @State private var selection: SidebarSelection? = .nav(.dashboard)
    @State private var columnVisibility: NavigationSplitViewVisibility = .automatic
    @State private var sidebarOutlineView: NSOutlineView?

    var body: some View {
        @Bindable var appState = appState

        NavigationSplitView(columnVisibility: $columnVisibility) {
            SidebarView(selection: $selection, onOutlineViewReady: { outlineView in
                sidebarOutlineView = outlineView
            })
                .navigationSplitViewColumnWidth(
                    min: PurePointTheme.sidebarMinWidth,
                    ideal: PurePointTheme.sidebarIdealWidth,
                    max: PurePointTheme.sidebarMaxWidth
                )
        } detail: {
            DetailView(selection: $selection)
        }
        .navigationTitle("")
        .overlay(alignment: .top) {
            if let error = appState.daemonError {
                DaemonErrorBanner(message: error) {
                    appState.daemonError = nil
                }
            }
        }
        .animation(.easeInOut(duration: 0.25), value: appState.daemonError)
        .overlay {
            if appState.showSettings {
                Color.black.opacity(0.3)
                    .ignoresSafeArea()
                    .onTapGesture { appState.showSettings = false }

                SettingsView()
                    .clipShape(RoundedRectangle(cornerRadius: 12))
                    .shadow(color: .black.opacity(0.3), radius: 20, y: 10)
                    .onExitCommand { appState.showSettings = false }
                    .transition(.opacity.combined(with: .scale(scale: 0.95)))
            }
        }
        .animation(.easeInOut(duration: 0.2), value: appState.showSettings)
        .onReceive(NotificationCenter.default.publisher(for: .hotkeyAction)) { notification in
            guard let action = notification.userInfo?["action"] as? HotkeyAction else { return }
            handleHotkeyAction(action)
        }
        .onChange(of: appState.pendingSelectAgentId) { _, agentId in
            guard let agentId else { return }
            appState.pendingSelectAgentId = nil
            appState.pendingFocusAgentId = agentId
            selection = .agent(agentId)
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.5) {
                appState.pendingFocusAgentId = nil
            }
        }
        .onChange(of: selection) { _, newValue in
            appState.activeSidebarSelection = newValue

            // Track active project for Cmd+N routing
            switch newValue {
            case .agent(let id):
                appState.activeProjectRoot = appState.projectState(forAgentId: id)?.projectRoot
            case .worktree(let id):
                appState.activeProjectRoot = appState.projectState(forWorktreeId: id)?.projectRoot
            case .project(let root):
                appState.activeProjectRoot = root
            default:
                break // keep last known project
            }

            guard case .agent(let agentId) = newValue else {
                // Non-agent selection (nav items, worktrees): deactivate grid
                if gridState.isActive { gridState.deactivate() }
                appState.selectedAgentId = nil
                return
            }

            // Clicking the grid owner while grid active → already showing it
            if gridState.isActive, agentId == gridState.ownerAgentId {
                appState.selectedAgentId = agentId
                return
            }

            // Clicking the grid owner while suspended → restore grid
            if gridState.restoreIfOwner(agentId) {
                appState.selectedAgentId = agentId
                return
            }

            // Clicking any other agent → deactivate grid
            if gridState.isActive {
                gridState.deactivate()
            }
            appState.selectedAgentId = agentId
        }
    }

    // MARK: - Hotkey Dispatch

    private func handleHotkeyAction(_ action: HotkeyAction) {
        switch action {
        case .focusSidebar:
            columnVisibility = .all
            DispatchQueue.main.async {
                if let outlineView = sidebarOutlineView {
                    outlineView.window?.makeFirstResponder(outlineView)
                }
            }

        case .focusContent:
            // Find the terminal in the current view and focus it
            DispatchQueue.main.async {
                guard let window = NSApp.keyWindow else { return }
                focusTerminalInWindow(window)
            }

        case .toggleSidebar:
            withAnimation {
                columnVisibility = columnVisibility == .detailOnly ? .all : .detailOnly
            }

        case .navDashboard:
            selection = .nav(.dashboard)

        case .navAgents:
            selection = .nav(.agents)

        case .navSchedule:
            selection = .nav(.schedule)

        case .closeAgent:
            if gridState.isActive {
                gridState.closeFocused()
            } else if let agentId = appState.selectedAgentId {
                let projectRoot = appState.projectState(forAgentId: agentId)?.projectRoot ?? ""
                appState.projectState(forRoot: projectRoot)?.removeAndKillAgent(agentId)
                selection = .nav(.dashboard)
            }

        case .toggleChatSidebar:
            NotificationCenter.default.post(name: .toggleChatSidebar, object: nil)

        default:
            break
        }
    }

    private func focusTerminalInWindow(_ window: NSWindow) {
        // Walk the view hierarchy to find a visible TerminalView
        func findTerminalView(in view: NSView) -> NSView? {
            if let pane = view as? TerminalPaneNSView,
               let tv = pane.terminal?.terminalView {
                return tv
            }
            for sub in view.subviews where !sub.isHidden {
                if let found = findTerminalView(in: sub) { return found }
            }
            return nil
        }

        guard let contentView = window.contentView,
              let tv = findTerminalView(in: contentView) else { return }
        window.makeFirstResponder(tv)
    }
}
