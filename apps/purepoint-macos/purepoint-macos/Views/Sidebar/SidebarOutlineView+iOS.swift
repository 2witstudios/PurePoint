#if os(iOS)
import SwiftUI

/// UIViewControllerRepresentable wrapping SidebarOutlineViewController: the iOS twin of the
/// Mac representable, bridging the same SwiftUI state to the `UICollectionView` outline.
struct SidebarOutlineView: UIViewControllerRepresentable {
    @Environment(SettingsState.self) private var settingsState
    @Binding var selection: SidebarSelection?
    var appState: AppState
    var registry: WorkspaceRegistry
    var viewCache: TerminalViewCache
    var onOutlineViewReady: ((UIView) -> Void)?

    func makeUIViewController(context: Context) -> SidebarOutlineViewController {
        let vc = SidebarOutlineViewController(expansionStore: SidebarExpansionStore())
        vc.outlineView.onWindowAttached = onOutlineViewReady

        Self.wireCallbacks(
            vc, selection: $selection, appState: appState, registry: registry, viewCache: viewCache,
            settingsState: settingsState)
        return vc
    }

    func updateUIViewController(_ vc: SidebarOutlineViewController, context: Context) {
        _ = appState.signalClock
        // The canonical workspace list is the sidebar's only input.
        vc.workspacesByProject = registry.workspacesByProject
        vc.activeWorkspaceId = registry.activeWorkspaceId

        // Rebuild tree when data changes
        vc.rebuildNodes(projects: appState.projects, machines: appState.machines)

        // Handle incoming programmatic selection (e.g., pendingSelectAgentId)
        vc.selectNode(for: selection)
    }
}
#endif
