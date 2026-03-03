import SwiftUI

struct ContentView: View {
    @Environment(AppState.self) private var appState
    @Environment(GridState.self) private var gridState
    @State private var selection: SidebarSelection?
    @State private var columnVisibility: NavigationSplitViewVisibility = .automatic

    var body: some View {
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
        .onChange(of: selection) { _, newValue in
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
