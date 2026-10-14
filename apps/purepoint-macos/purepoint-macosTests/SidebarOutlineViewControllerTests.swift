import AppKit
import Foundation
import Testing

@testable import PurePoint

private struct SidebarTestWorkspaceService: WorkspaceService {
    func loadWorkspace(projectRoot: String) async throws -> WorkspaceSnapshot {
        WorkspaceSnapshot(worktrees: [], rootAgents: [])
    }

    func manifestPath(projectRoot: String) -> String { "/dev/null" }
}

@MainActor
private func makeSidebarAgent(id: String) -> AgentModel {
    AgentModel(id: id, name: id, agentType: "claude", status: .running, prompt: "", startedAt: "")
}

@MainActor
private func makeSidebarWorkspace(agentId: String, in container: WorkspaceContainer) -> Workspace {
    Workspace.adopting(agentId: agentId, container: container)
}

@MainActor
private func makeSidebarWorktree(_ id: String) -> WorktreeModel {
    makeSidebarWorktree(id, branch: "branch-\(id)")
}

@MainActor
private func makeSidebarWorktree(_ id: String, branch: String) -> WorktreeModel {
    WorktreeModel(
        id: id,
        name: id,
        path: "/tmp/wt-\(id)",
        branch: branch,
        status: "active",
        agents: [makeSidebarAgent(id: "agent-\(id)")]
    )
}

@MainActor
private func makeSidebarProject(root: String, worktreeCount: Int) -> ProjectState {
    let project = ProjectState(projectRoot: root, service: SidebarTestWorkspaceService(), registry: nil)
    project.worktrees = (0..<worktreeCount).map { index in
        WorktreeModel(
            id: "wt-\(index)",
            name: "wt-\(index)",
            path: "/tmp/wt-\(index)",
            branch: "branch-\(index)",
            status: "active",
            agents: [makeSidebarAgent(id: "agent-\(index)")]
        )
    }
    return project
}

@Suite(.serialized)
@MainActor
struct SidebarOutlineViewControllerTests {

    /// A store over a private suite, so a test's rows never reach the real defaults.
    private func makeExpansionStore() throws -> (store: SidebarExpansionStore, suite: String) {
        let suite = "pp-sidebar-expansion-\(UUID().uuidString)"
        let defaults = try #require(UserDefaults(suiteName: suite))
        return (SidebarExpansionStore(defaults: defaults), suite)
    }

    /// A controller whose expansion state lives in a private suite.
    private func makeSidebarController() throws -> (controller: SidebarOutlineViewController, suite: String) {
        let (store, suite) = try makeExpansionStore()
        let controller = SidebarOutlineViewController(expansionStore: store)
        controller.loadViewIfNeeded()
        return (controller, suite)
    }

    /// Every row key in a subtree, so two subtrees can be compared for collisions.
    private func rowKeys(_ node: SidebarNode) -> Set<String> {
        var keys: Set<String> = [node.rowKey]
        for child in node.children {
            keys.formUnion(rowKeys(child))
        }
        return keys
    }

    @Test func givenEmptyProjectShouldHaveNoSidebarChildren() throws {
        let (controller, _) = try makeSidebarController()
        controller.rebuildNodes(projects: [makeSidebarProject(root: "/tmp/empty", worktreeCount: 0)])

        #expect(controller.projectNodes.count == 1)
        #expect(controller.projectNodes[0].children.isEmpty)
    }

    @Test func givenProjectWithWorktreesShouldOnlyShowWorktreeChildren() throws {
        let (controller, _) = try makeSidebarController()
        controller.rebuildNodes(projects: [makeSidebarProject(root: "/tmp/project", worktreeCount: 2)])

        #expect(controller.projectNodes[0].children.map(\.id) == ["wt-0", "wt-1"])
    }

    @Test func givenUnreadChannelChangesShouldRefreshProjectBadgeWithoutChildRows() async throws {
        let (controller, _) = try makeSidebarController()
        let root = "/tmp/sidebar-unread-\(UUID().uuidString)"
        let defaults = try #require(UserDefaults(suiteName: root))
        defer { defaults.removePersistentDomain(forName: root) }
        let message = ChannelMessage(
            id: "message", sequence: 1, parentId: nil,
            author: ChannelAuthor(
                id: "other", name: "Other", kind: "human", agentType: nil, worktreeId: nil, branch: nil),
            text: "Update", createdAt: "", editedAt: nil, references: [], reactions: []
        )
        let channel = ChannelState(projectRoot: root, defaults: defaults) { _ in
            .channelHistory(
                ChannelHistory(
                    messages: [message], revision: 1, latestSequence: 1,
                    hasMore: false, oldestSequence: 1, unchanged: false,
                    selfAuthorId: "self", replyCounts: [:]
                ))
        }
        let project = ProjectState(
            projectRoot: root, service: SidebarTestWorkspaceService(), registry: nil, channel: channel)
        controller.rebuildNodes(projects: [project])
        let initialNode = try #require(controller.projectNodes.first)

        await channel.refresh()
        #expect(channel.unreadCount == 1)
        controller.rebuildNodes(projects: [project])
        let unreadNode = try #require(controller.projectNodes.first)
        #expect(unreadNode !== initialNode)
        #expect(unreadNode.children.isEmpty)
        let cell = try #require(controller.outlineView(controller.outlineView, viewFor: nil, item: unreadNode))
        let badge = try #require(
            cell.subviews.flatMap { $0.subviews }.compactMap { $0 as? NSTextField }
                .first { $0.identifier?.rawValue == "channelUnreadCount" })
        #expect(badge.stringValue == "1")

        channel.markRead()
        controller.rebuildNodes(projects: [project])
        let readNode = try #require(controller.projectNodes.first)
        #expect(readNode !== unreadNode)
        let readCell = try #require(controller.outlineView(controller.outlineView, viewFor: nil, item: readNode))
        #expect(!readCell.subviews.flatMap { $0.subviews }.contains { $0.identifier?.rawValue == "channelUnreadCount" })
    }

    @Test func givenScrolledSidebarShouldPreserveScrollPositionAcrossUnchangedRebuild() throws {
        let (controller, _) = try makeSidebarController()
        controller.view.frame = NSRect(x: 0, y: 0, width: 260, height: 160)
        controller.view.layoutSubtreeIfNeeded()

        let project = makeSidebarProject(root: "/tmp/project", worktreeCount: 30)
        controller.rebuildNodes(projects: [project])
        controller.outlineView.layoutSubtreeIfNeeded()

        let scrollPoint = NSPoint(x: 0, y: 180)
        controller.scrollView.contentView.scroll(to: scrollPoint)
        controller.scrollView.reflectScrolledClipView(controller.scrollView.contentView)
        let startingOffset = controller.scrollView.contentView.bounds.origin.y

        #expect(startingOffset > 0)

        controller.rebuildNodes(projects: [project])
        controller.outlineView.layoutSubtreeIfNeeded()

        #expect(controller.scrollView.contentView.bounds.origin.y == startingOffset)
    }

    // MARK: - Row expansion

    @Test func givenARowTheUserClosedShouldStayClosedWhenAnUnrelatedRefreshRebuildsTheTree() throws {
        let (controller, _) = try makeSidebarController()
        let closed = makeSidebarProject(root: "/tmp/sidebar-closed-\(UUID().uuidString)", worktreeCount: 2)
        let unrelated = makeSidebarProject(root: "/tmp/sidebar-unrelated-\(UUID().uuidString)", worktreeCount: 1)
        controller.rebuildNodes(projects: [closed, unrelated])
        controller.outlineView.layoutSubtreeIfNeeded()
        #expect(controller.outlineView.isItemExpanded(controller.projectNodes[0]))

        controller.outlineView.collapseItem(controller.projectNodes[0])

        // An unrelated refresh: another project's worktree is renamed.
        unrelated.worktrees = [makeSidebarWorktree("wt-0", branch: "renamed")]
        controller.rebuildNodes(projects: [closed, unrelated])
        controller.outlineView.layoutSubtreeIfNeeded()

        #expect(!controller.outlineView.isItemExpanded(controller.projectNodes[0]))
        #expect(controller.outlineView.isItemExpanded(controller.projectNodes[1]))
    }

    @Test func givenARowTheUserClosedShouldKeepItsStateWhenItsParentClosesAndReopens() throws {
        let (controller, _) = try makeSidebarController()
        let project = makeSidebarProject(root: "/tmp/sidebar-parent-\(UUID().uuidString)", worktreeCount: 2)
        controller.rebuildNodes(projects: [project])
        controller.outlineView.layoutSubtreeIfNeeded()

        let closedWorktree = controller.projectNodes[0].children[0]
        controller.outlineView.collapseItem(closedWorktree)
        controller.outlineView.collapseItem(controller.projectNodes[0])

        // Closing the project must not discard what its rows were doing.
        project.worktrees = [
            makeSidebarWorktree("wt-0", branch: "renamed"), makeSidebarWorktree("wt-1"),
        ]
        controller.rebuildNodes(projects: [project])
        controller.outlineView.layoutSubtreeIfNeeded()
        #expect(!controller.outlineView.isItemExpanded(controller.projectNodes[0]))

        controller.outlineView.expandItem(controller.projectNodes[0])
        controller.outlineView.layoutSubtreeIfNeeded()

        #expect(!controller.outlineView.isItemExpanded(controller.projectNodes[0].children[0]))
        #expect(controller.outlineView.isItemExpanded(controller.projectNodes[0].children[1]))
    }

    @Test func givenANewRowShouldAppearExpandedBesideTheRowsTheUserLeftAlone() throws {
        let (controller, _) = try makeSidebarController()
        let project = makeSidebarProject(root: "/tmp/sidebar-new-row-\(UUID().uuidString)", worktreeCount: 1)
        controller.rebuildNodes(projects: [project])
        controller.outlineView.layoutSubtreeIfNeeded()
        controller.outlineView.collapseItem(controller.projectNodes[0].children[0])

        project.worktrees.append(makeSidebarWorktree("wt-added"))
        controller.rebuildNodes(projects: [project])
        controller.outlineView.layoutSubtreeIfNeeded()

        #expect(!controller.outlineView.isItemExpanded(controller.projectNodes[0].children[0]))
        #expect(controller.outlineView.isItemExpanded(controller.projectNodes[0].children[1]))
    }

    @Test func givenASelectionInsideAClosedRowShouldOpenItsAncestorsAndSelectTheRow() throws {
        let (controller, _) = try makeSidebarController()
        let project = makeSidebarProject(root: "/tmp/sidebar-select-\(UUID().uuidString)", worktreeCount: 1)
        let workspace = makeSidebarWorkspace(agentId: "ag-1", in: .worktree("wt-0"))
        controller.workspacesByProject = [project.projectId: [workspace]]
        controller.rebuildNodes(projects: [project])
        controller.outlineView.layoutSubtreeIfNeeded()
        controller.outlineView.collapseItem(controller.projectNodes[0])

        controller.selectNode(for: .workspace(workspace.id))
        controller.outlineView.layoutSubtreeIfNeeded()

        let workspaceRow = controller.outlineView.row(
            forItem: try #require(controller.projectNodes[0].children[0].children.first))
        #expect(workspaceRow >= 0)
        #expect(controller.outlineView.selectedRow == workspaceRow)
    }

    @Test func givenAProjectThatHasNotPublishedItsRowsShouldKeepWhatItRecorded() throws {
        let suite = "pp-sidebar-expansion-\(UUID().uuidString)"
        let defaults = try #require(UserDefaults(suiteName: suite))
        defer { defaults.removePersistentDomain(forName: suite) }
        let root = "/tmp/sidebar-unpublished-\(UUID().uuidString)"
        let worktreeKey = SidebarNode.rowKey("worktree", "wt-0", under: SidebarNode.rowKey("project", root))
        // A previous launch closed a worktree row under this project.
        let store = SidebarExpansionStore(defaults: defaults)
        store.setExpanded(worktreeKey, isExpanded: false)
        let controller = SidebarOutlineViewController(expansionStore: store)
        controller.loadViewIfNeeded()

        // The relaunch order: the project row is here, its snapshot has not been read, so no
        // rows have been published for it and its worktrees are not in the tree yet.
        controller.rebuildNodes(projects: [makeSidebarProject(root: root, worktreeCount: 0)])

        // The snapshot lands. The row the user closed arrives, and must still be closed —
        // the rebuild above must not have taken the tree as complete.
        controller.workspacesByProject = [root: [makeSidebarWorkspace(agentId: "ag-1", in: .worktree("wt-0"))]]
        controller.rebuildNodes(projects: [makeSidebarProject(root: root, worktreeCount: 1)])

        #expect(store.closedRowKeys == [worktreeKey])
    }

    @Test func givenARowClosedBeforeALaunchShouldStayClosedOnTheFirstBuild() throws {
        let (controller, suite) = try makeSidebarController()
        defer { UserDefaults(suiteName: suite)?.removePersistentDomain(forName: suite) }
        let root = "/tmp/sidebar-launch-\(UUID().uuidString)"
        controller.rebuildNodes(projects: [makeSidebarProject(root: root, worktreeCount: 1)])
        controller.outlineView.layoutSubtreeIfNeeded()
        controller.outlineView.collapseItem(controller.projectNodes[0])

        // The next launch reads what the last one recorded.
        let relaunched = SidebarOutlineViewController(
            expansionStore: SidebarExpansionStore(defaults: UserDefaults(suiteName: suite)!))
        relaunched.loadViewIfNeeded()
        relaunched.rebuildNodes(projects: [makeSidebarProject(root: root, worktreeCount: 1)])
        relaunched.outlineView.layoutSubtreeIfNeeded()

        #expect(!relaunched.outlineView.isItemExpanded(relaunched.projectNodes[0]))
    }

    @Test func givenAProjectRootThatSpellsOutAnotherRowsKeyShouldNotShareIt() {
        // A folder literally named after a worktree row under another project: without an
        // escape both rows would answer to the same key, so closing one would close both.
        let worktreeRow = SidebarNode.rowKey("worktree", "wt-abc12345", under: SidebarNode.rowKey("project", "/repo"))
        let lookalike = SidebarNode.projectNode(
            makeSidebarProject(root: "/repo/worktree:wt-abc12345", worktreeCount: 0), workspaces: []
        ).rowKey
        #expect(lookalike != worktreeRow)
    }

    @Test func givenTheSameRootOnTwoMachinesShouldGiveEveryRowItsOwnKey() {
        let machineId = UUID()
        let project = makeSidebarProject(root: "/srv/app", worktreeCount: 1)

        // Two machines hosting a project at the same root: the same rows, two rows each.
        let first = SidebarNode.projectNode(project, workspaces: [], parentKey: SidebarNode.machineKey(machineId))
        let second = SidebarNode.projectNode(
            project, workspaces: [], parentKey: SidebarNode.machineKey(UUID()))

        #expect(rowKeys(first).isDisjoint(with: rowKeys(second)))
        #expect(rowKeys(first).count == 2)
    }
}
