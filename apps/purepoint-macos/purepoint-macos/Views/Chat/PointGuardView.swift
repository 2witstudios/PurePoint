import SwiftUI

/// Point Guard is a root workspace, using the same tabs, splits and palette as agents.
struct PointGuardView: View {
    @Binding var selection: SidebarSelection?
    @Environment(AppState.self) private var appState
    @Environment(WorkspaceRegistry.self) private var registry

    private var projectRoot: String? {
        appState.activeProjectRoot.flatMap { appState.projectState(forRoot: $0) != nil ? $0 : nil }
            ?? appState.projects.first?.projectRoot
    }

    private var workspace: Workspace? {
        projectRoot.flatMap { root in registry.workspacesByProject[root]?.first(where: \.isPointGuard) }
    }

    var body: some View {
        Group {
            if let projectRoot, let error = registry.layoutErrorsByProject[projectRoot] {
                ContentUnavailableView {
                    Label("Could not restore layout", systemImage: "exclamationmark.triangle")
                } description: {
                    Text(error)
                } actions: {
                    Button("Retry", action: activateWorkspace)
                }
            } else if let workspace {
                PaneGridView(workspaceId: workspace.id)
            } else if projectRoot == nil {
                VStack(spacing: 0) {
                    PointGuardPaneControls(model: appState.pointGuardChat)
                        .frame(maxWidth: .infinity, alignment: .trailing).padding(8)
                    PointGuardChatSurfaceView(isFocused: true)
                }
            } else {
                Color.clear
            }
        }
        .onAppear(perform: activateWorkspace)
        .onChange(of: projectRoot) { _, _ in activateWorkspace() }
        .task { appState.pointGuardService.start(chat: appState.pointGuardChat) }
    }

    private func activateWorkspace() {
        guard let projectRoot else { return }
        guard let id = try? registry.ensurePointGuardWorkspace(projectRoot: projectRoot) else {
            registry.activeWorkspaceId = nil
            return
        }
        registry.activate(workspaceId: id)
    }
}

struct PointGuardChatSurfaceView: View {
    let isFocused: Bool
    @Environment(AppState.self) private var appState
    @AppStorage("PP_pointGuardShowConversations") private var showSidebar = false

    var body: some View {
        PiPointGuardChatView(model: appState.pointGuardChat, showSidebar: $showSidebar, isFocused: isFocused)
            .onReceive(NotificationCenter.default.publisher(for: .toggleChatSidebar)) { _ in showSidebar.toggle() }
    }
}
