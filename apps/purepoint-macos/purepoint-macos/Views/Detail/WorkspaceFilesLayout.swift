import SwiftUI

/// Keep the pane grid's identity stable when the inspector is hidden or resized.
struct WorkspaceFilesLayout<Content: View>: View {
    let rootPath: String
    let host: any ProjectHost
    @ViewBuilder let content: () -> Content
    @AppStorage("showWorkspaceFilesSidebar") private var showSidebar = true
    @State private var filesState: WorkspaceFilesState

    init(rootPath: String, host: any ProjectHost, @ViewBuilder content: @escaping () -> Content) {
        self.rootPath = rootPath
        self.host = host
        self.content = content
        _filesState = State(initialValue: WorkspaceFilesState(rootPath: rootPath, host: host))
    }

    var body: some View {
        HSplitView {
            content()
                .frame(minWidth: 160, maxWidth: .infinity, maxHeight: .infinity)
                .layoutPriority(1)
            if showSidebar {
                WorkspaceFilesSidebar(state: filesState)
                    .id(filesState.identity)
                    .frame(minWidth: 240, idealWidth: 360, maxWidth: 640, maxHeight: .infinity)
            }
        }
        // Keyed on host and path: a local and a remote root can share a path.
        .onChange(of: WorkspaceFilesState.identity(rootPath: rootPath, host: host)) { _, _ in
            filesState = WorkspaceFilesState(rootPath: rootPath, host: host)
        }
        // No `.toolbar` here: any toolbar gives the window a title-bar band above the pane tab strips.
        // View > Toggle Workspace Sidebar is the control.
    }
}
