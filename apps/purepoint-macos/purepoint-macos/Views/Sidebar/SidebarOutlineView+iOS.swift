#if os(iOS)
import SwiftUI

/// iOS twin of the Mac sidebar representable, with the same inputs. It hosts the
/// `SidebarOutlineViewController` placeholder until the real list lands (IPAD-6.2).
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
        vc.onSelectionChanged = { newSelection in
            selection = newSelection
        }
        return vc
    }

    func updateUIViewController(_ vc: SidebarOutlineViewController, context: Context) {
        _ = appState.signalClock
        vc.workspacesByProject = registry.workspacesByProject
        vc.activeWorkspaceId = registry.activeWorkspaceId
        vc.rebuildNodes(projects: appState.projects, machines: appState.machines)
        vc.selectNode(for: selection)
    }
}
#endif
