import Foundation
import Testing

@testable import PurePoint

/// The tab operations on `Workspace`: every one is pure, keeps each agent in exactly one
/// tab, and never leaves a pane with no tabs.
@Suite struct PaneTabStackTests {

    /// One pane (leaf 0) holding tabs A, B, C, with B active.
    private func stacked() -> Workspace {
        var workspace = Workspace.adopting(agentId: "A", container: .projectRoot)
        workspace.newTab(leafId: 0, content: .agent("B"))
        workspace.newTab(leafId: 0, content: .agent("C"))
        workspace.selectTab(leafId: 0, index: 1)
        return workspace
    }

    private func labels(_ workspace: Workspace, _ leafId: Int) -> [String] {
        workspace.panes[leafId]?.tabs.map { surface in
            switch surface.content {
            case .agent(let id): id
            case .file(let path): path ?? "files"
            case .empty: "+"
            }
        } ?? []
    }

    private func activeLabel(_ workspace: Workspace, _ leafId: Int) -> String? {
        workspace.panes[leafId]?.activeTab?.content.agentId
    }

    // MARK: - New tab

    @Test func newTabInsertsAfterTheActiveTabAndActivatesIt() {
        var workspace = stacked()
        let id = workspace.newTab(leafId: 0)

        #expect(labels(workspace, 0) == ["A", "B", "+", "C"])
        #expect(workspace.panes[0]?.activeTabId == id)
        #expect(workspace.focusedLeafId == 0)
    }

    @Test func newTabIdsAreUniqueAcrossPanes() {
        var workspace = stacked()
        workspace.split(leafId: 0, axis: .vertical)
        workspace.newTab(leafId: 1)

        let ids = workspace.surfaces.map(\.surface.id)
        #expect(Set(ids).count == ids.count)
    }

    @Test func newTabInMissingPaneDoesNothing() {
        var workspace = stacked()
        #expect(workspace.newTab(leafId: 9) == nil)
        #expect(workspace == stacked())
    }

    // MARK: - Select

    @Test func selectByPositionIgnoresOutOfRange() {
        var workspace = stacked()
        workspace.selectTab(leafId: 0, index: 7)
        #expect(activeLabel(workspace, 0) == "B")
        workspace.selectTab(leafId: 0, index: 0)
        #expect(activeLabel(workspace, 0) == "A")
    }

    @Test func selectLastTab() {
        var workspace = stacked()
        workspace.selectLastTab(leafId: 0)
        #expect(activeLabel(workspace, 0) == "C")
    }

    @Test func cycleWrapsAtBothEnds() {
        var workspace = stacked()
        workspace.cycleTab(leafId: 0, by: 1)
        #expect(activeLabel(workspace, 0) == "C")
        workspace.cycleTab(leafId: 0, by: 1)
        #expect(activeLabel(workspace, 0) == "A")
        workspace.cycleTab(leafId: 0, by: -1)
        #expect(activeLabel(workspace, 0) == "C")
    }

    @Test func selectingATabFocusesItsPane() {
        var workspace = stacked()
        workspace.split(leafId: 0, axis: .vertical)
        #expect(workspace.focusedLeafId == 1)

        workspace.selectTab(workspace.location(ofAgent: "C")!.surfaceId)

        #expect(workspace.focusedLeafId == 0)
        #expect(workspace.focusedAgentId == "C")
    }

    // MARK: - Close

    @Test func closingTheActiveTabActivatesItsLeftNeighbour() {
        var workspace = stacked()
        let killed = workspace.closeTab(workspace.location(ofAgent: "B")!.surfaceId)

        #expect(killed == ["B"])
        #expect(labels(workspace, 0) == ["A", "C"])
        #expect(activeLabel(workspace, 0) == "A")
    }

    @Test func closingTheLeftmostActiveTabActivatesTheNewFirstTab() {
        var workspace = stacked()
        workspace.selectTab(leafId: 0, index: 0)
        workspace.closeTab(workspace.location(ofAgent: "A")!.surfaceId)
        #expect(activeLabel(workspace, 0) == "B")
    }

    @Test func closingABackgroundTabKeepsTheActiveOne() {
        var workspace = stacked()
        workspace.closeTab(workspace.location(ofAgent: "C")!.surfaceId)
        #expect(activeLabel(workspace, 0) == "B")
    }

    @Test func closingAPanesLastTabCollapsesThePane() {
        var workspace = stacked()
        workspace.split(leafId: 0, axis: .vertical, content: .agent("D"))

        let killed = workspace.closeTab(workspace.location(ofAgent: "D")!.surfaceId)

        #expect(killed == ["D"])
        #expect(workspace.paneCount == 1)
        #expect(workspace.panes.keys.sorted() == [0])
        #expect(workspace.focusedLeafId == 0)
    }

    @Test func closingAPaneReturnsEveryAgentInIt() {
        var workspace = stacked()
        workspace.split(leafId: 0, axis: .vertical, content: .agent("D"))
        workspace.newTab(leafId: 1, content: .file(path: nil))

        let killed = workspace.closePane(leafId: 0)

        #expect(killed == ["A", "B", "C"])
        #expect(workspace.root == .leaf(id: 1))
        #expect(workspace.agentIds == ["D"])
    }

    // MARK: - Move

    @Test func moveTabToAnotherPaneKeepsItsIdAndActivatesIt() {
        var workspace = stacked()
        workspace.split(leafId: 0, axis: .vertical, content: .agent("D"))
        let c = workspace.location(ofAgent: "C")!.surfaceId

        workspace.moveTab(c, toLeaf: 1, index: 0)

        #expect(labels(workspace, 0) == ["A", "B"])
        #expect(labels(workspace, 1) == ["C", "D"])
        #expect(workspace.panes[1]?.activeTabId == c)
        #expect(workspace.focusedLeafId == 1)
    }

    @Test func moveTabWithoutIndexAppends() {
        var workspace = stacked()
        workspace.split(leafId: 0, axis: .vertical, content: .agent("D"))
        workspace.moveTab(workspace.location(ofAgent: "A")!.surfaceId, toLeaf: 1)
        #expect(labels(workspace, 1) == ["D", "A"])
    }

    @Test func moveTabReordersWithinAPane() {
        var workspace = stacked()
        workspace.moveTab(workspace.location(ofAgent: "C")!.surfaceId, toLeaf: 0, index: 0)
        #expect(labels(workspace, 0) == ["C", "A", "B"])
    }

    @Test func movingAPanesOnlyTabCollapsesTheSourcePane() {
        var workspace = stacked()
        workspace.split(leafId: 0, axis: .vertical, content: .agent("D"))

        workspace.moveTab(workspace.location(ofAgent: "D")!.surfaceId, toLeaf: 0)

        #expect(workspace.paneCount == 1)
        #expect(labels(workspace, 0) == ["A", "B", "C", "D"])
    }

    @Test func moveBeforeLaterTabInSamePaneLandsJustBeforeIt() {
        var workspace = stacked()
        let a = workspace.location(ofAgent: "A")!.surfaceId
        let c = workspace.location(ofAgent: "C")!.surfaceId
        workspace.moveTab(a, before: c)
        #expect(labels(workspace, 0) == ["B", "A", "C"])
    }

    @Test func moveBeforeEarlierTabInSamePane() {
        var workspace = stacked()
        workspace.moveTab(workspace.location(ofAgent: "C")!.surfaceId, before: workspace.location(ofAgent: "A")!.surfaceId)
        #expect(labels(workspace, 0) == ["C", "A", "B"])
    }

    @Test func moveBeforeTabInAnotherPane() {
        var workspace = stacked()
        workspace.split(leafId: 0, axis: .vertical, content: .agent("D"))
        workspace.moveTab(workspace.location(ofAgent: "B")!.surfaceId, before: workspace.location(ofAgent: "D")!.surfaceId)
        #expect(labels(workspace, 0) == ["A", "C"])
        #expect(labels(workspace, 1) == ["B", "D"])
    }

    @Test func moveBeforeItselfDoesNothing() {
        var workspace = stacked()
        let b = workspace.location(ofAgent: "B")!.surfaceId
        workspace.moveTab(b, before: b)
        #expect(workspace == stacked())
    }

    @Test func moveTabToMissingPaneDoesNothing() {
        var workspace = stacked()
        workspace.moveTab(0, toLeaf: 9)
        #expect(workspace == stacked())
    }

    // MARK: - Break out / join

    @Test func breakTabMovesItIntoANewPaneBesideItsOwn() {
        var workspace = stacked()
        let b = workspace.location(ofAgent: "B")!.surfaceId

        let newLeaf = workspace.breakTab(b, axis: .vertical)

        #expect(newLeaf == 1)
        #expect(workspace.root == .split(axis: .vertical, ratio: 0.5, first: .leaf(id: 0), second: .leaf(id: 1)))
        #expect(labels(workspace, 0) == ["A", "C"])
        #expect(labels(workspace, 1) == ["B"])
        #expect(workspace.focusedAgentId == "B")
    }

    @Test func breakingAPanesOnlyTabDoesNothing() {
        var workspace = Workspace.adopting(agentId: "A", container: .projectRoot)
        #expect(workspace.breakTab(0, axis: .vertical) == nil)
        #expect(workspace.paneCount == 1)
    }

    @Test func breakTabRespectsTheSplitLimit() {
        var workspace = stacked()
        for _ in 0..<5 { workspace.split(leafId: 0, axis: .vertical) }
        #expect(workspace.paneCount == 6)
        #expect(workspace.breakTab(workspace.location(ofAgent: "B")!.surfaceId, axis: .vertical) == nil)
        #expect(labels(workspace, 0) == ["A", "B", "C"])
    }

    // MARK: - Ghosts

    @Test func aWorkspaceWithOnlyEmptyTabsIsAGhost() {
        var workspace = stacked()
        #expect(!workspace.isGhost)
        _ = workspace.removeSurfaces { $0.content.agentId != nil }
        workspace.normalize()
        #expect(workspace.isGhost)
    }

    @Test func aFileTabKeepsAWorkspaceAlive() {
        var workspace = Workspace.adopting(agentId: "A", container: .projectRoot)
        workspace.newTab(leafId: 0, content: .file(path: "/tmp/x.md"))
        workspace.closeTab(0)
        #expect(workspace.agentIds.isEmpty)
        #expect(!workspace.isGhost)
    }

    // MARK: - Content and pruning

    @Test func setContentReplacesOnlyThatTab() {
        var workspace = stacked()
        let id = workspace.newTab(leafId: 0)!
        workspace.setContent(.agent("E"), forSurface: id)
        #expect(labels(workspace, 0) == ["A", "B", "E", "C"])
    }

    @Test func removeSurfacesReportsWhenNothingIsLeft() {
        var workspace = stacked()
        #expect(workspace.removeSurfaces { $0.content.agentId == "B" })
        #expect(labels(workspace, 0) == ["A", "C"])
        #expect(!workspace.removeSurfaces { _ in true })
    }

    // MARK: - Normalize

    @Test func normalizeGivesMissingPanesAnEmptyTabAndFixesDuplicateIds() {
        var workspace = Workspace(
            id: "ws", container: .projectRoot,
            root: .split(axis: .vertical, ratio: 0.5, first: .leaf(id: 0), second: .leaf(id: 1)),
            panes: [
                0: Pane(tabs: [Surface(id: 3, content: .agent("A")), Surface(id: 3, content: .agent("B"))], activeTabId: 3),
                7: Pane(tabs: [Surface(id: 9, content: .agent("Z"))], activeTabId: 9),
            ],
            focusedLeafId: 5, nextLeafId: 0)
        workspace.normalize()

        let ids = workspace.surfaces.map(\.surface.id)
        #expect(Set(ids).count == ids.count)
        #expect(workspace.panes.keys.sorted() == [0, 1])
        #expect(workspace.panes[1]?.tabs.map(\.content) == [.empty])
        #expect(workspace.focusedLeafId == 0)
        #expect(workspace.nextLeafId == 2)
        #expect(workspace.nextSurfaceId > (ids.max() ?? 0))
    }
}
