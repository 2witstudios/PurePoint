#if os(macOS)
import SwiftUI

/// NSViewControllerRepresentable wrapping SidebarOutlineViewController.
/// Bridges SwiftUI state (selection, appState, workspace registry) to the AppKit outline view.
struct SidebarOutlineView: NSViewControllerRepresentable {
    @Environment(SettingsState.self) private var settingsState
    @Binding var selection: SidebarSelection?
    var appState: AppState
    var registry: WorkspaceRegistry
    var viewCache: TerminalViewCache
    var onOutlineViewReady: ((NSOutlineView) -> Void)?

    func makeNSViewController(context: Context) -> SidebarOutlineViewController {
        let vc = SidebarOutlineViewController(expansionStore: SidebarExpansionStore())
        vc.outlineView.onWindowAttached = onOutlineViewReady

        Self.wireCallbacks(
            vc, selection: $selection, appState: appState, registry: registry, viewCache: viewCache,
            settingsState: settingsState)
        return vc
    }

    func updateNSViewController(_ vc: SidebarOutlineViewController, context: Context) {
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
