#if os(iOS)
import UIKit

/// Placeholder twin of the Mac sidebar outline controller, with the same callbacks and inputs.
/// It keeps the node tree the Mac builds but renders only a label until the `UICollectionView`
/// list lands (IPAD-6.2).
class SidebarOutlineViewController: UIViewController {
    let outlineView = SidebarKeyOutlineView()

    var projectNodes: [SidebarNode] = []

    var onSelectionChanged: ((SidebarSelection?) -> Void)?
    var onShowCommandPalette: ((ProjectState, SidebarSelection?, Bool) -> Void)?
    var onAddTerminal: ((ProjectState, WorktreeModel) -> Void)?
    var onKillWorkspace: ((ProjectState, String) -> Void)?
    var onKillWorktreeAgents: ((ProjectState, String) -> Void)?
    var onRenameAgent: ((ProjectState, String, String) -> Void)?
    var onDeleteWorktree: ((ProjectState, String) -> Void)?
    var onKillAllProjectAgents: ((ProjectState) -> Void)?
    var onRemoveProject: ((ProjectState) -> Void)?
    var onRemoveMachine: ((MachineState) -> Void)?

    var workspacesByProject: [String: [Workspace]] = [:]
    var activeWorkspaceId: String?

    private let expansionStore: SidebarExpansionStore

    init(expansionStore: SidebarExpansionStore) {
        self.expansionStore = expansionStore
        super.init(nibName: nil, bundle: nil)
    }

    required init?(coder: NSCoder) {
        fatalError("init(coder:) is not supported: the sidebar is built in code")
    }

    override func loadView() {
        view = outlineView
        NotYetOnIPad.addLabel("SidebarOutlineViewController", to: outlineView)
    }

    /// Builds the same node tree as the Mac: projects, then a MACHINES section.
    func rebuildNodes(projects: [ProjectState], machines: [MachineState] = []) {
        projectNodes = projects.map { SidebarNode.projectNode($0, workspaces: workspacesByProject[$0.projectId] ?? []) }
        if !machines.isEmpty {
            projectNodes.append(SidebarNode.sectionHeaderNode("MACHINES"))
            for machine in machines {
                let machineKey = SidebarNode.machineKey(machine.machine.id)
                projectNodes.append(
                    SidebarNode(
                        kind: .machine(machine),
                        rowKey: machineKey,
                        children: machine.projects.map {
                            SidebarNode.projectNode(
                                $0, workspaces: workspacesByProject[$0.projectId] ?? [], parentKey: machineKey)
                        }))
            }
        }
    }

    func selectNode(for selection: SidebarSelection?) {}

    func toggleSelectedMachineConnection() -> Bool { false }
}

/// Set while a sidebar row is being renamed so terminals don't take first responder
/// away from the field editor when the selection switches content.
enum InlineRenameFocus {
    static var isActive = false
}

/// Placeholder twin of the Mac outline view: the view the sidebar focuses.
final class SidebarKeyOutlineView: UIView {
    /// Return pressed; true if handled.
    var onReturn: (() -> Bool)?
    /// Called once this view has joined a window, so a focus request made before that can land.
    var onWindowAttached: ((UIView) -> Void)?

    override var canBecomeFirstResponder: Bool { true }

    override func didMoveToWindow() {
        super.didMoveToWindow()
        guard window != nil else { return }
        onWindowAttached?(self)
    }
}
#endif
