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

    @Test func givenEmptyProjectShouldHaveNoSidebarChildren() {
        let controller = SidebarOutlineViewController()
        controller.loadViewIfNeeded()
        controller.rebuildNodes(projects: [makeSidebarProject(root: "/tmp/empty", worktreeCount: 0)])

        #expect(controller.projectNodes.count == 1)
        #expect(controller.projectNodes[0].children.isEmpty)
    }

    @Test func givenProjectWithWorktreesShouldOnlyShowWorktreeChildren() {
        let controller = SidebarOutlineViewController()
        controller.loadViewIfNeeded()
        controller.rebuildNodes(projects: [makeSidebarProject(root: "/tmp/project", worktreeCount: 2)])

        #expect(controller.projectNodes[0].children.map(\.id) == ["wt-0", "wt-1"])
    }

    @Test func givenUnreadChannelChangesShouldRefreshProjectBadgeWithoutChildRows() async throws {
        let root = "/tmp/sidebar-unread-\(UUID().uuidString)"
        let defaults = try #require(UserDefaults(suiteName: root))
        defer { defaults.removePersistentDomain(forName: root) }
        let message = ChannelMessage(
            id: "message", sequence: 1, parentId: nil,
            author: ChannelAuthor(id: "other", name: "Other", kind: "human", agentType: nil, worktreeId: nil, branch: nil),
            text: "Update", createdAt: "", editedAt: nil, references: [], reactions: []
        )
        let channel = ChannelState(projectRoot: root, defaults: defaults) { _ in
            .channelHistory(ChannelHistory(
                messages: [message], revision: 1, latestSequence: 1,
                hasMore: false, oldestSequence: 1, unchanged: false,
                selfAuthorId: "self", replyCounts: [:]
            ))
        }
        let project = ProjectState(projectRoot: root, service: SidebarTestWorkspaceService(), registry: nil, channel: channel)
        let controller = SidebarOutlineViewController()
        controller.loadViewIfNeeded()
        controller.rebuildNodes(projects: [project])
        let initialNode = try #require(controller.projectNodes.first)

        await channel.refresh()
        #expect(channel.unreadCount == 1)
        controller.rebuildNodes(projects: [project])
        let unreadNode = try #require(controller.projectNodes.first)
        #expect(unreadNode !== initialNode)
        #expect(unreadNode.children.isEmpty)
        let cell = try #require(controller.outlineView(controller.outlineView, viewFor: nil, item: unreadNode))
        let badge = try #require(cell.subviews.flatMap { $0.subviews }.compactMap { $0 as? NSTextField }
            .first { $0.identifier?.rawValue == "channelUnreadCount" })
        #expect(badge.stringValue == "1")

        channel.markRead()
        controller.rebuildNodes(projects: [project])
        let readNode = try #require(controller.projectNodes.first)
        #expect(readNode !== unreadNode)
        let readCell = try #require(controller.outlineView(controller.outlineView, viewFor: nil, item: readNode))
        #expect(!readCell.subviews.flatMap { $0.subviews }.contains { $0.identifier?.rawValue == "channelUnreadCount" })
    }

    @Test func givenScrolledSidebarShouldPreserveScrollPositionAcrossUnchangedRebuild() {
        let controller = SidebarOutlineViewController()
        controller.loadViewIfNeeded()
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
}
