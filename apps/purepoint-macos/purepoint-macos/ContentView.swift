import SwiftUI

struct ContentView: View {
    @Environment(AppState.self) private var appState
    @Environment(GridState.self) private var gridState
    @State private var selection: SidebarSelection?
    @State private var columnVisibility: NavigationSplitViewVisibility = .automatic

    var body: some View {
        @Bindable var appState = appState

        NavigationSplitView(columnVisibility: $columnVisibility) {
            SidebarView(selection: $selection)
                .navigationSplitViewColumnWidth(
                    min: PurePointTheme.sidebarMinWidth,
                    ideal: PurePointTheme.sidebarIdealWidth,
                    max: PurePointTheme.sidebarMaxWidth
                )
        } detail: {
            DetailView(selection: selection)
        }
        .navigationTitle("PurePoint")
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
                // Non-agent selection (nav items, worktrees): suspend grid if active
                if gridState.isActive { gridState.suspend() }
                appState.selectedAgentId = nil
                return
            }

            // Clicking the grid owner → restore grid
            if gridState.restoreIfOwner(agentId) {
                appState.selectedAgentId = agentId
                return
            }

            // Clicking any other agent → suspend grid, show single-pane
            if gridState.isActive {
                gridState.suspend()
            }
            appState.selectedAgentId = agentId
        }
    }
}
