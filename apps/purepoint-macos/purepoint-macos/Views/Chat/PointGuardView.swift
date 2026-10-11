import SwiftUI

/// Point Guard is a root workspace, using the same tabs, splits and palette as agents.
struct PointGuardView: View {
    @Binding var selection: SidebarSelection?
    @Environment(AppState.self) private var appState
    @Environment(WorkspaceRegistry.self) private var registry

    private var projectRoot: String { appState.pointGuardRoot }

    private var workspace: Workspace? {
        registry.workspacesByProject[projectRoot]?.first(where: \.isPointGuard)
    }

    var body: some View {
        Group {
            if let error = registry.layoutErrorsByProject[projectRoot] {
                ContentUnavailableView {
                    Label("Could not restore layout", systemImage: "exclamationmark.triangle")
                } description: {
                    Text(error)
                } actions: {
                    Button("Retry", action: activateWorkspace)
                }
            } else if let workspace {
                PaneGridView(workspaceId: workspace.id)
            } else {
                Color.clear
            }
        }
        .onAppear(perform: activateWorkspace)
        .onChange(of: projectRoot) { _, _ in activateWorkspace() }
        .task { appState.pointGuardService.start(chat: appState.pointGuardChat) }
    }

    private func activateWorkspace() {
        appState.activatePointGuardWorkspace()
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
