import SwiftUI

/// Keep the pane grid's identity stable when the inspector is hidden or resized.
struct WorkspaceFilesLayout<Content: View>: View {
    let rootPath: String
    let host: any ProjectHost
    @ViewBuilder let content: () -> Content
    @AppStorage(SidebarVisibility.workspaceSidebarKey) private var showSidebar = true
    @State private var filesState: WorkspaceFilesState

    init(rootPath: String, host: any ProjectHost, @ViewBuilder content: @escaping () -> Content) {
        self.rootPath = rootPath
        self.host = host
        self.content = content
        _filesState = State(initialValue: WorkspaceFilesState(rootPath: rootPath, host: host))
    }

    var body: some View {
        Group {
            #if os(macOS)
            HSplitView {
                content()
                    .frame(minWidth: 160, maxWidth: .infinity, maxHeight: .infinity)
                    .layoutPriority(1)
                    // While hidden, the top-right pane's tab bar carries the button that reopens it.
                    .environment(\.showsWorkspaceSidebarToggle, !showSidebar)
                if showSidebar {
                    WorkspaceFilesSidebar(state: filesState)
                        .id(filesState.identity)
                        .frame(minWidth: 240, idealWidth: 360, maxWidth: 640, maxHeight: .infinity)
                }
            }
            #else
            // The same limits, with a divider the iPad can drag (the iOS `HSplitView` has none).
            WorkspaceFilesSplit(showSidebar: showSidebar) {
                content()
                    .environment(\.showsWorkspaceSidebarToggle, !showSidebar)
            } sidebar: {
                WorkspaceFilesSidebar(state: filesState)
                    .id(filesState.identity)
            }
            #endif
        }
        // Keyed on host and path: a local and a remote root can share a path.
        .onChange(of: WorkspaceFilesState.identity(rootPath: rootPath, host: host)) { _, _ in
            filesState = WorkspaceFilesState(rootPath: rootPath, host: host)
        }
        // No `.toolbar` here: any toolbar gives the window a title-bar band above the pane tab strips.
        // The toggle lives in the top strip instead: the sidebar's header, or the pane tab bar while hidden.
    }
}

/// The button that shows or hides the workspace sidebar, at the trailing end of the top strip.
struct WorkspaceSidebarToggle: View {
    @AppStorage(SidebarVisibility.workspaceSidebarKey) private var showSidebar = true

    var body: some View {
        TabBarButton(
            icon: "sidebar.right", help: showSidebar ? "Hide Workspace Sidebar" : "Show Workspace Sidebar"
        ) {
            withAnimation { showSidebar.toggle() }
        }
    }
}

private struct ShowsWorkspaceSidebarToggleKey: EnvironmentKey {
    static let defaultValue = false
}

extension EnvironmentValues {
    /// True inside a workspace whose sidebar is hidden, so a pane tab bar can offer to reopen it.
    var showsWorkspaceSidebarToggle: Bool {
        get { self[ShowsWorkspaceSidebarToggleKey.self] }
        set { self[ShowsWorkspaceSidebarToggleKey.self] = newValue }
    }
}
