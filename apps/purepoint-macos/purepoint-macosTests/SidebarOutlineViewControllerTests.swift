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
    let project = ProjectState(projectRoot: root, service: SidebarTestWorkspaceService(), gridState: nil)
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
