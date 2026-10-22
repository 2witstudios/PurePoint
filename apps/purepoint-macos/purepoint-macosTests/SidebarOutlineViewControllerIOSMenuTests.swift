#if os(iOS)
import Foundation
import Testing
import UIKit

@testable import PurePoint

// The iPad sidebar's context menus, drags and inline rename.

private struct IOSSidebarTestWorkspaceService: WorkspaceService {
    func loadWorkspace(projectRoot: String) async throws -> WorkspaceSnapshot {
        WorkspaceSnapshot(worktrees: [], rootAgents: [])
    }

    func manifestPath(projectRoot: String) -> String { "/dev/null" }
}

@MainActor
private func makeWorktree(_ id: String, branch: String? = nil) -> WorktreeModel {
    WorktreeModel(
        id: id, name: id, path: "/tmp/wt-\(id)", branch: branch ?? "branch-\(id)", status: "active",
        agents: [AgentModel(id: "agent-\(id)", name: "agent-\(id)", agentType: "claude", status: .running, prompt: "", startedAt: "")])
}

/// The machine every test project lives on, so a root names the same project each time.
private let testMachineId = UUID()

@MainActor
private func makeProject(root: String, worktreeCount: Int) -> ProjectState {
    let project = ProjectState(
        projectRoot: root, service: IOSSidebarTestWorkspaceService(), registry: nil, endpoint: .machine(testMachineId),
        host: UnavailableProjectHost())
    project.worktrees = (0..<worktreeCount).map { makeWorktree("\($0)") }
    return project
}

@Suite(.serialized)
@MainActor
struct SidebarOutlineViewControllerIOSMenuTests {

    /// A controller in a window, its expansion state in a private suite.
    private func makeController(
        store: SidebarExpansionStore? = nil
    ) throws -> (controller: SidebarOutlineViewController, window: UIWindow) {
        let suite = "pp-sidebar-ios-\(UUID().uuidString)"
        let defaults = try #require(UserDefaults(suiteName: suite))
        let controller = SidebarOutlineViewController(expansionStore: store ?? SidebarExpansionStore(defaults: defaults))
        let scene = try #require(UIApplication.shared.connectedScenes.lazy.compactMap { $0 as? UIWindowScene }.first)
        let window = UIWindow(windowScene: scene)
        window.frame = CGRect(x: 0, y: 0, width: 260, height: 400)
        window.rootViewController = controller
        window.makeKeyAndVisible()
        controller.view.layoutIfNeeded()
        return (controller, window)
    }

    private func visibleIds(_ controller: SidebarOutlineViewController) -> [String] {
        let view = controller.outlineView
        view.layoutIfNeeded()
        return (0..<view.numberOfItems(inSection: 0)).compactMap { item in
            guard let source = view.dataSource as? UICollectionViewDiffableDataSource<Int, String>,
                let key = source.itemIdentifier(for: IndexPath(item: item, section: 0))
            else { return nil }
            return key
        }
    }

    private func key(_ node: SidebarNode) -> String { node.rowKey }

    @Test func givenAWorktreeRowShouldOfferTheMacContextMenu() throws {
        let (controller, _) = try makeController()
        let project = makeProject(root: "/tmp/ios-menu-\(UUID().uuidString)", worktreeCount: 1)
        controller.rebuildNodes(projects: [project])

        let menu = controller.makeMenu(
            SidebarMenus.contextMenu(for: controller.projectNodes[0].children[0]),
            key: key(controller.projectNodes[0].children[0]))
        let groups = menu.children.compactMap { $0 as? UIMenu }
        #expect(groups.map { $0.children.compactMap { ($0 as? UIAction)?.title } } == [
            ["Kill All Agents (1)"], ["Delete Worktree\u{2026}"],
        ])
        let config = controller.collectionView(
            controller.outlineView, contextMenuConfigurationForItemsAt: [IndexPath(item: 1, section: 0)], point: .zero)
        #expect(config != nil)
    }

    @Test func givenTheMacSidebarHasNoDragsShouldOfferNone() throws {
        let (controller, _) = try makeController()
        #expect(!controller.outlineView.dragInteractionEnabled)
        #expect(controller.outlineView.dragDelegate == nil)
    }

    @Test func givenARenameShouldRenameTheWorkspacesAgentAndHoldRebuildsUntilItEnds() async throws {
        let (controller, _) = try makeController()
        let project = makeProject(root: "/tmp/ios-rename-\(UUID().uuidString)", worktreeCount: 1)
        let workspace = Workspace.adopting(agentId: "agent-0", container: .worktree("0"))
        controller.workspacesByProject = [project.projectId: [workspace]]
        controller.rebuildNodes(projects: [project])
        var renamed: [(String, String)] = []
        controller.onRenameAgent = { _, agentId, name in renamed.append((agentId, name)) }

        controller.beginRename(controller.projectNodes[0].children[0].children[0])
        let field = try #require(controller.editingTextField)
        #expect(field.text == "agent-0")
        #expect(InlineRenameFocus.isActive)

        // A refresh mid-rename waits for the rename to end.
        project.worktrees.append(makeWorktree("1"))
        controller.rebuildNodes(projects: [project])
        #expect(controller.projectNodes[0].children.count == 1)

        field.text = "  reviewer  "
        controller.textFieldDidEndEditing(field)

        #expect(renamed.map(\.0) == ["agent-0"])
        #expect(renamed.map(\.1) == ["reviewer"])
        #expect(controller.editingTextField == nil)
        #expect(!InlineRenameFocus.isActive)
        #expect(controller.projectNodes[0].children.count == 2)
    }

    @Test func givenAnEscapedRenameShouldKeepTheName() throws {
        let (controller, _) = try makeController()
        let project = makeProject(root: "/tmp/ios-escape-\(UUID().uuidString)", worktreeCount: 1)
        let workspace = Workspace.adopting(agentId: "agent-0", container: .worktree("0"))
        controller.workspacesByProject = [project.projectId: [workspace]]
        controller.rebuildNodes(projects: [project])
        var renamed = 0
        controller.onRenameAgent = { _, _, _ in renamed += 1 }

        controller.beginRename(controller.projectNodes[0].children[0].children[0])
        let field = try #require(controller.editingTextField)
        field.text = "other"
        field.onCancel?()

        #expect(renamed == 0)
        #expect(controller.editingTextField == nil)
        let cell = try #require(controller.outlineView.cellForItem(at: IndexPath(item: 2, section: 0)) as? SidebarRowCell)
        #expect(cell.nameLabel?.text == "agent-0")
    }
}
#endif
