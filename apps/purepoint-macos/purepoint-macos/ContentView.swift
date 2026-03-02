import SwiftUI

struct ContentView: View {
    @Environment(AppState.self) private var appState
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
    }
}
