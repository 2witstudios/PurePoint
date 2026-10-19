import SwiftUI

/// Point Guard is a root workspace, using the same tabs, splits and palette as agents.
struct PointGuardView: View {
    @Binding var selection: SidebarSelection?
    @Environment(AppState.self) private var appState
    @Environment(WorkspaceRegistry.self) private var registry
    @AppStorage("PP_pointGuardShowConversations") private var showSidebar = false

    private var projectRoot: String? {
        guard let root = appState.pointGuardRoot, !appState.isTooBroadProjectRoot(root) else { return nil }
        return root
    }

    private var workspace: Workspace? {
        projectRoot.flatMap { registry.workspacesByProject[$0]?.first(where: \.isPointGuard) }
    }

    private var hasVisibleChat: Bool {
        projectRoot == nil || workspace?.panes.values.contains(where: { $0.activeTab?.content == .pointGuard }) == true
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
                PointGuardChatSurfaceView(isFocused: true, tabBar: AnyView(controlsStrip))
                    .background(Color(nsColor: Theme.chatBackground))
            } else {
                Color.clear
            }
        }
        .onAppear(perform: activateWorkspace)
        .onChange(of: projectRoot) { _, _ in activateWorkspace() }
        .task { appState.pointGuardService.start(chat: appState.pointGuardChat) }
        .onReceive(NotificationCenter.default.publisher(for: .toggleChatSidebar)) { _ in
            // The workspace owns this shared setting once, even when a terminal
            // or file sibling has focus. Background chat tabs don't handle it.
            if hasVisibleChat { showSidebar.toggle() }
        }
    }

    /// Without a workspace there is no pane tab strip, so the controls take its place beside the
    /// conversation rail's header. A separate row above the chat would push the rail below the top edge.
    private var controlsStrip: some View {
        PointGuardPaneControls(model: appState.pointGuardChat)
            .padding(.horizontal, 8)
            // Matches PaneTabBar.height and the rail header, so the two read as one row.
            .frame(maxWidth: .infinity, minHeight: 30, maxHeight: 30, alignment: .trailing)
            .background(Color(nsColor: Theme.tabBarBackground))
            .overlay(alignment: .bottom) { Divider() }
            .environment(\.colorScheme, .dark)
    }

    private func activateWorkspace() {
        appState.activatePointGuardWorkspace()
    }
}

struct PointGuardChatSurfaceView: View {
    let isFocused: Bool
    var tabBar: AnyView = AnyView(EmptyView())
    @Environment(AppState.self) private var appState
    @AppStorage("PP_pointGuardShowConversations") private var showSidebar = false

    var body: some View {
        PiPointGuardChatView(
            model: appState.pointGuardChat, showSidebar: $showSidebar, isFocused: isFocused, tabBar: tabBar)
    }
}
