import SwiftUI
import AppKit

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
        .background(WindowConfigurator())
    }
}

/// Configures the NSWindow to blend the title bar with the sidebar material.
private struct WindowConfigurator: NSViewRepresentable {
    func makeNSView(context: Context) -> NSView {
        let view = NSView()
        DispatchQueue.main.async {
            guard let window = view.window else { return }
            window.titlebarAppearsTransparent = true
            window.titlebarSeparatorStyle = .none
        }
        return view
    }

    func updateNSView(_ nsView: NSView, context: Context) {}
}

#Preview {
    ContentView()
        .preferredColorScheme(.dark)
}
