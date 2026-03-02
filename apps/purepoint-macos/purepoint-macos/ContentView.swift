import SwiftUI

struct ContentView: View {
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
    }
}
