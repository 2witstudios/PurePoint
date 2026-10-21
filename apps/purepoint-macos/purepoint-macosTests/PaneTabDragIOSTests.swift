#if os(iOS)
import Foundation
import Testing
import UIKit

@testable import PurePoint

private struct TabDragStubService: WorkspaceService {
    func loadWorkspace(projectRoot: String) async throws -> WorkspaceSnapshot {
        WorkspaceSnapshot(worktrees: [], rootAgents: [])
    }
    func manifestPath(projectRoot: String) -> String { "/dev/null" }
}

@MainActor @Suite
struct PaneTabDragIOSTests {
    private let root = NSTemporaryDirectory() + "pp-tab-drag-tests-" + UUID().uuidString

    private func makeRegistry() -> WorkspaceRegistry {
        let registry = WorkspaceRegistry()
        registry.reconcile(
            projectRoot: root,
            rootAgents: [AgentModel(id: "ag-a", name: "ag-a", agentType: "claude", status: .running, prompt: "", startedAt: "")],
            worktrees: [])
        return registry
    }

    @Test func dropResolvesTheDraggedTabButNotTheTargetItself() {
        let five = TabDragPayload.string(for: 5)
        #expect(TabDragPayload.movedSurface(in: [five], onto: 2) == 5)
        #expect(TabDragPayload.movedSurface(in: [five], onto: 5) == nil)
        #expect(TabDragPayload.movedSurface(in: ["plain text", five], onto: nil) == 5)
        #expect(TabDragPayload.movedSurface(in: ["plain text"], onto: 2) == nil)
    }

    /// A tab dropped on another tab of the same pane is inserted before it, through the same
    /// registry call the Mac's drop destination makes.
    @Test func droppingOnATabReordersThroughTheRegistry() {
        let registry = makeRegistry()
        let workspaceId = "ws-ag-a"
        let first = registry.workspace(id: workspaceId)!.panes[0]!.tabs[0].id
        let second = registry.newTab(workspaceId: workspaceId, leafId: 0)!
        let view = TabInteractionView()
        view.surfaceId = first
        view.onDrop = { moved in registry.moveTab(workspaceId: workspaceId, surfaceId: moved, before: first) }

        view.onDrop(TabDragPayload.movedSurface(in: [TabDragPayload.string(for: second)], onto: first)!)

        #expect(registry.workspace(id: workspaceId)!.panes[0]!.tabs.map(\.id) == [second, first])
    }

    /// Dropping a tab onto another pane's tab moves it into that split.
    @Test func droppingOnAnotherPanesTabMovesItIntoThatSplit() {
        let registry = makeRegistry()
        let workspaceId = "ws-ag-a"
        let moving = registry.newTab(workspaceId: workspaceId, leafId: 0)!
        registry.split(workspaceId: workspaceId, leafId: 0, axis: .vertical)
        let workspace = registry.workspace(id: workspaceId)!
        let otherLeaf = workspace.root.allLeafIds.first { $0 != 0 }!
        let target = workspace.panes[otherLeaf]!.tabs[0].id

        registry.moveTab(workspaceId: workspaceId, surfaceId: moving, before: target)

        let moved = registry.workspace(id: workspaceId)!
        #expect(moved.panes[otherLeaf]!.tabs.map(\.id) == [moving, target])
        #expect(!moved.panes[0]!.tabs.map(\.id).contains(moving))
    }

    @Test func touchesOnAVisibleCloseButtonPassThroughToIt() {
        let view = TabInteractionView()
        view.frame = CGRect(x: 0, y: 0, width: 120, height: PaneTabBar.height)
        let close = CGPoint(x: view.closeButtonFrame.midX, y: view.closeButtonFrame.midY)
        view.closeVisible = false
        #expect(view.hitTest(close, with: nil) === view)
        view.closeVisible = true
        #expect(view.hitTest(close, with: nil) == nil)
        #expect(view.hitTest(CGPoint(x: 20, y: 15), with: nil) === view)
    }

    @Test func dragPreviewIsTheTitlePaddedBySix() {
        let preview = TabInteractionView.previewLabel("Point Guard")
        let label = UILabel()
        label.text = "Point Guard"
        label.font = .systemFont(ofSize: 12)
        label.sizeToFit()
        #expect(preview.bounds.width == label.bounds.width + 12)
        #expect(preview.bounds.height == label.bounds.height + 12)
    }
}
#endif
