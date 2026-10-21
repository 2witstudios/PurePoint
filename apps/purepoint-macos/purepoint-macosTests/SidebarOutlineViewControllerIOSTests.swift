#if os(iOS)
import Foundation
import Testing
import UIKit

@testable import PurePoint

// The iPad sidebar outline: the same tree, expansion and selection behaviour the Mac outline
// tests check, plus the keyboard on the UIKit list.

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
struct SidebarOutlineViewControllerIOSTests {

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

    @Test func givenProjectsShouldListEveryRowOpenByDefault() throws {
        let (controller, _) = try makeController()
        let project = makeProject(root: "/tmp/ios-list-\(UUID().uuidString)", worktreeCount: 2)
        let workspace = Workspace.adopting(agentId: "agent-0", container: .worktree("0"))
        controller.workspacesByProject = [project.projectId: [workspace]]
        controller.rebuildNodes(projects: [project])

        let nodes = controller.projectNodes
        #expect(
            visibleIds(controller)
                == [key(nodes[0]), key(nodes[0].children[0]), key(nodes[0].children[0].children[0]), key(nodes[0].children[1])])
    }

    @Test func givenMachinesShouldShowTheMachinesSectionAndMachineRows() throws {
        let (controller, _) = try makeController()
        let machine = MachineState(machine: Machine(name: "studio", sshHost: "me@studio"), appState: nil, registry: nil)
        controller.rebuildNodes(projects: [], machines: [machine])

        #expect(visibleIds(controller) == [SidebarNode.rowKey("section", "MACHINES"), SidebarNode.machineKey(machine.id)])
        let header = try #require(controller.outlineView.cellForItem(at: IndexPath(item: 0, section: 0)))
        #expect(header.accessibilityLabel == "Machines")
        let row = try #require(controller.outlineView.cellForItem(at: IndexPath(item: 1, section: 0)))
        #expect(row.accessibilityLabel?.hasPrefix("studio (me@studio): ") == true)
        #expect(row.accessibilityCustomActions?.map(\.name) == ["Connect", "Remove Machine"])
    }

    @Test func givenARowTheUserClosedShouldStayClosedWhenAnUnrelatedRefreshRebuildsTheTree() throws {
        let (controller, _) = try makeController()
        let closed = makeProject(root: "/tmp/ios-closed-\(UUID().uuidString)", worktreeCount: 2)
        let unrelated = makeProject(root: "/tmp/ios-unrelated-\(UUID().uuidString)", worktreeCount: 1)
        controller.rebuildNodes(projects: [closed, unrelated])
        controller.setExpanded(key(controller.projectNodes[0]), false, animated: false)

        unrelated.worktrees = [makeWorktree("0", branch: "renamed")]
        controller.rebuildNodes(projects: [closed, unrelated])

        #expect(!controller.isExpanded(key(controller.projectNodes[0])))
        #expect(controller.isExpanded(key(controller.projectNodes[1])))
        #expect(!visibleIds(controller).contains(key(controller.projectNodes[0].children[0])))
    }

    @Test func givenARowTheUserClosedShouldKeepItsStateWhenItsParentClosesAndReopens() throws {
        let (controller, _) = try makeController()
        let project = makeProject(root: "/tmp/ios-parent-\(UUID().uuidString)", worktreeCount: 2)
        let workspaces = [
            Workspace.adopting(agentId: "agent-0", container: .worktree("0")),
            Workspace.adopting(agentId: "agent-1", container: .worktree("1")),
        ]
        controller.workspacesByProject = [project.projectId: workspaces]
        controller.rebuildNodes(projects: [project])
        let projectKey = key(controller.projectNodes[0])
        let closedWorktree = key(controller.projectNodes[0].children[0])
        let openWorktree = key(controller.projectNodes[0].children[1])

        controller.setExpanded(closedWorktree, false, animated: false)
        controller.setExpanded(projectKey, false, animated: false)
        controller.setExpanded(projectKey, true, animated: false)

        #expect(!controller.isExpanded(closedWorktree))
        #expect(controller.isExpanded(openWorktree))
        #expect(visibleIds(controller).contains(key(controller.projectNodes[0].children[1].children[0])))
        #expect(!visibleIds(controller).contains(key(controller.projectNodes[0].children[0].children[0])))
    }

    @Test func givenARowClosedBeforeALaunchShouldStayClosedOnTheFirstBuild() throws {
        let suite = "pp-sidebar-ios-launch-\(UUID().uuidString)"
        let defaults = try #require(UserDefaults(suiteName: suite))
        defer { defaults.removePersistentDomain(forName: suite) }
        let root = "/tmp/ios-launch-\(UUID().uuidString)"
        let (controller, _) = try makeController(store: SidebarExpansionStore(defaults: defaults))
        controller.rebuildNodes(projects: [makeProject(root: root, worktreeCount: 1)])
        controller.setExpanded(key(controller.projectNodes[0]), false, animated: false)

        let (relaunched, _) = try makeController(store: SidebarExpansionStore(defaults: defaults))
        relaunched.rebuildNodes(projects: [makeProject(root: root, worktreeCount: 1)])

        #expect(!relaunched.isExpanded(key(relaunched.projectNodes[0])))
    }

    @Test func givenASettledTreeShouldDropRowsThatAreGone() throws {
        let suite = "pp-sidebar-ios-prune-\(UUID().uuidString)"
        let defaults = try #require(UserDefaults(suiteName: suite))
        defer { defaults.removePersistentDomain(forName: suite) }
        let root = "/tmp/ios-prune-\(UUID().uuidString)"
        let goneKey = SidebarNode.rowKey("worktree", "wt-gone", under: SidebarNode.rowKey("project", root))
        let store = SidebarExpansionStore(defaults: defaults)
        store.setExpanded(goneKey, isExpanded: false)
        let (controller, _) = try makeController(store: store)

        // Not loaded yet: an empty project is no evidence the row is gone.
        let project = makeProject(root: root, worktreeCount: 0)
        controller.rebuildNodes(projects: [project])
        #expect(store.closedRowKeys == [goneKey])

        project.markWorktreesLoadedForTesting()
        controller.rebuildNodes(projects: [project])
        #expect(store.closedRowKeys.isEmpty)
    }

    @Test func givenASelectionInsideAClosedRowShouldOpenItsAncestorsAndSelectTheRow() throws {
        let (controller, _) = try makeController()
        let project = makeProject(root: "/tmp/ios-select-\(UUID().uuidString)", worktreeCount: 1)
        let workspace = Workspace.adopting(agentId: "agent-0", container: .worktree("0"))
        controller.workspacesByProject = [project.projectId: [workspace]]
        controller.rebuildNodes(projects: [project])
        controller.setExpanded(key(controller.projectNodes[0]), false, animated: false)
        var reported: [SidebarSelection?] = []
        controller.onSelectionChanged = { reported.append($0) }

        controller.selectNode(for: .workspace(workspace.id))

        let selected = try #require(controller.outlineView.indexPathsForSelectedItems?.first)
        #expect(visibleIds(controller)[selected.item] == key(controller.projectNodes[0].children[0].children[0]))
        // A programmatic selection is not reported back.
        #expect(reported.isEmpty)

        controller.selectNode(for: .nav(.agents))
        #expect(controller.outlineView.indexPathsForSelectedItems?.isEmpty ?? true)
    }

    @Test func givenARowTappedShouldReportItsSelection() throws {
        let (controller, _) = try makeController()
        let project = makeProject(root: "/tmp/ios-tap-\(UUID().uuidString)", worktreeCount: 1)
        controller.rebuildNodes(projects: [project])
        var reported: [SidebarSelection?] = []
        controller.onSelectionChanged = { reported.append($0) }

        let indexPath = IndexPath(item: 1, section: 0)
        let view = controller.outlineView
        #expect(controller.collectionView(view, shouldSelectItemAt: indexPath))
        view.selectItem(at: indexPath, animated: false, scrollPosition: [])
        controller.collectionView(view, didSelectItemAt: indexPath)

        #expect(reported == [.worktree("0")])
        // Clicking the selected row again changes nothing.
        #expect(!controller.collectionView(view, shouldSelectItemAt: indexPath))
    }

    @Test func givenArrowKeysShouldMoveTheSelectionAndOpenAndCloseRows() throws {
        let (controller, _) = try makeController()
        let project = makeProject(root: "/tmp/ios-keys-\(UUID().uuidString)", worktreeCount: 1)
        let machine = MachineState(machine: Machine(name: "m", sshHost: "m"), appState: nil, registry: nil)
        controller.rebuildNodes(projects: [project], machines: [machine])
        var reported: [SidebarSelection?] = []
        controller.onSelectionChanged = { reported.append($0) }
        let projectKey = key(controller.projectNodes[0])

        #expect(controller.handleArrowKey(.down))
        #expect(reported == [.project(project.projectId)])

        #expect(controller.handleArrowKey(.left))
        #expect(!controller.isExpanded(projectKey))
        #expect(controller.handleArrowKey(.right))
        #expect(controller.isExpanded(projectKey))

        #expect(controller.handleArrowKey(.down))
        #expect(reported.last == .worktree("0"))
        // Left closes the open worktree row, then climbs to its parent.
        #expect(controller.handleArrowKey(.left))
        #expect(controller.handleArrowKey(.left))
        #expect(reported.last == .project(project.projectId))

        // Down passes the worktree, then skips the MACHINES title to reach the machine row.
        #expect(controller.handleArrowKey(.down))
        #expect(controller.handleArrowKey(.down))
        let selected = try #require(controller.outlineView.indexPathsForSelectedItems?.first)
        #expect(visibleIds(controller)[selected.item] == SidebarNode.machineKey(machine.id))
        // A machine row reports no selection; Return acts on it instead.
        #expect(reported.last == .worktree("0"))
    }

    @Test func givenReturnOnARowThatIsNotAMachineShouldLeaveTheKeyUnhandled() throws {
        let (controller, _) = try makeController()
        controller.rebuildNodes(projects: [makeProject(root: "/tmp/ios-return-\(UUID().uuidString)", worktreeCount: 0)])
        controller.selectNode(for: .project(controller.projectNodes[0].id))
        #expect(!controller.toggleSelectedMachineConnection())
    }

    @Test func givenScrolledSidebarShouldPreserveScrollPositionAcrossARebuild() throws {
        let (controller, _) = try makeController()
        let project = makeProject(root: "/tmp/ios-scroll-\(UUID().uuidString)", worktreeCount: 30)
        controller.rebuildNodes(projects: [project])
        controller.outlineView.layoutIfNeeded()
        controller.outlineView.contentOffset = CGPoint(x: 0, y: 180)
        let starting = controller.outlineView.contentOffset.y

        project.worktrees[3] = makeWorktree("3", branch: "renamed")
        controller.rebuildNodes(projects: [project])

        #expect(controller.outlineView.contentOffset.y == starting)
    }

    // MARK: - Key navigation model

    @Test func givenNoSelectionShouldSelectTheFirstOrLastSelectableRow() {
        let rows: [SidebarKeyNavigation.Row] = [
            .init(isSelectable: false), .init(), .init(parent: 1), .init(isSelectable: false),
        ]
        #expect(SidebarKeyNavigation.command(for: .down, rows: rows, selected: nil) == .select(1))
        #expect(SidebarKeyNavigation.command(for: .up, rows: rows, selected: nil) == .select(2))
        #expect(SidebarKeyNavigation.command(for: .down, rows: rows, selected: 2) == .none)
        #expect(SidebarKeyNavigation.command(for: .up, rows: rows, selected: 1) == .none)
        #expect(SidebarKeyNavigation.command(for: .left, rows: rows, selected: nil) == .none)
    }

    @Test func givenLeftAndRightShouldCloseOpenAndClimbAsTheMacOutlineDoes() {
        let rows: [SidebarKeyNavigation.Row] = [
            .init(isExpandable: true, isExpanded: true), .init(parent: 0, isExpandable: true, isExpanded: false),
        ]
        #expect(SidebarKeyNavigation.command(for: .left, rows: rows, selected: 0) == .collapse(0))
        #expect(SidebarKeyNavigation.command(for: .right, rows: rows, selected: 0) == .none)
        #expect(SidebarKeyNavigation.command(for: .right, rows: rows, selected: 1) == .expand(1))
        #expect(SidebarKeyNavigation.command(for: .left, rows: rows, selected: 1) == .select(0))
    }
}
#endif
