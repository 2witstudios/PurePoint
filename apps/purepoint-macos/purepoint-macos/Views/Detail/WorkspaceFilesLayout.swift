import SwiftUI

/// Keep the pane grid's identity stable when the inspector is hidden or resized.
struct WorkspaceFilesLayout<Content: View>: View {
    let rootPath: String
    @ViewBuilder let content: () -> Content
    @AppStorage("showWorkspaceFilesSidebar") private var showSidebar = true
    @State private var filesState: WorkspaceFilesState

    init(rootPath: String, @ViewBuilder content: @escaping () -> Content) {
        self.rootPath = rootPath
        self.content = content
        _filesState = State(initialValue: WorkspaceFilesState(rootPath: rootPath))
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
            filesState = WorkspaceFilesState(rootPath: path)
        }
        .toolbar {
            ToolbarItem(placement: .primaryAction) {
                Button {
                    showSidebar.toggle()
                } label: {
                    Image(systemName: "sidebar.right")
                }
                .help(showSidebar ? "Hide changes and files" : "Show changes and files")
                .accessibilityLabel(showSidebar ? "Hide changes and files" : "Show changes and files")
            }
        }
    }
}
