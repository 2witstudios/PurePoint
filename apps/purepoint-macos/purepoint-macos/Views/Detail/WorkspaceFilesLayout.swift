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
                    .id(filesState.rootPath)
                    .frame(minWidth: 240, idealWidth: 360, maxWidth: 640, maxHeight: .infinity)
            }
        }
        .onChange(of: rootPath) { _, path in
            filesState = WorkspaceFilesState(rootPath: path, host: host)
        }
        .toolbar {
            ToolbarItem(placement: .primaryAction) {
                Button {
                    showSidebar.toggle()
                } label: {
                    Image(systemName: "sidebar.right")
                }
                .help(showSidebar ? "Hide workspace sidebar" : "Show workspace sidebar")
                .accessibilityLabel(showSidebar ? "Hide workspace sidebar" : "Show workspace sidebar")
            }
        }
    }
}
