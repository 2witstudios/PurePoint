import Foundation
import Testing

@testable import PurePoint

/// A file is a kind of tab: it sits in a pane's stack beside agent tabs, and is
/// persisted inside the leaf that holds it.
@Suite struct FilePaneTests {
    /// Pane 0 holds agent "a" (surface 0); pane 1 holds one empty tab (surface 1).
    private func twoPaneWorkspace() -> Workspace {
        Workspace(
            id: "ws-a", container: .projectRoot,
            root: .split(axis: .vertical, ratio: 0.5, first: .leaf(id: 0), second: .leaf(id: 1)),
            panes: [
                0: Pane(tabs: [Surface(id: 0, content: .agent("a"))], activeTabId: 0),
                1: Pane(tabs: [Surface(id: 1, content: .empty)], activeTabId: 1),
            ],
            focusedLeafId: 1, nextLeafId: 2, nextSurfaceId: 2)
    }

    @Test func givenEmptyTabOpeningFileRecordsPath() {
        var ws = twoPaneWorkspace()
        ws.setContent(.file(path: "/tmp/x.md"), forSurface: 1)
        #expect(ws.surface(id: 1)?.content == .file(path: "/tmp/x.md"))
    }

    @Test func givenAgentPaneFileOpensAsNewTabBesideIt() {
        var ws = twoPaneWorkspace()
        let fileTab = ws.newTab(leafId: 0, content: .file(path: nil))
        #expect(ws.panes[0]?.tabs.map(\.content) == [.agent("a"), .file(path: nil)])
        #expect(ws.panes[0]?.activeTabId == fileTab)
        #expect(ws.agentIds == ["a"])
    }

    @Test func givenFileTabClosedItsPaneCollapses() {
        var ws = twoPaneWorkspace()
        ws.setContent(.file(path: nil), forSurface: 1)
        let killed = ws.closeTab(1)
        #expect(killed.isEmpty)
        #expect(ws.paneCount == 1)
        #expect(ws.panes[1] == nil)
    }

    @Test func reconcileKeepsFileTabNextToLiveAgent() {
        var ws = twoPaneWorkspace()
        ws.setContent(.file(path: "/tmp/x.md"), forSurface: 1)
        let result = WorkspaceReconciler.reconcile(
            stored: [ws], live: [LiveAgent(id: "a", container: .projectRoot)])
        #expect(result.count == 1)
        #expect(result[0].surface(id: 1)?.content == .file(path: "/tmp/x.md"))
    }

    @Test func persistenceRoundTripsFileTabs() throws {
        var ws = twoPaneWorkspace()
        ws.setContent(.file(path: "/tmp/x.md"), forSurface: 1)
        let root = NSTemporaryDirectory() + "pu-filepane-\(UUID().uuidString)"
        defer { try? FileManager.default.removeItem(atPath: root) }
        WorkspacePersistence.save([ws], projectRoot: root)
        let loaded = WorkspacePersistence.load(projectRoot: root)
        #expect(loaded.first?.surface(id: 1)?.content == .file(path: "/tmp/x.md"))
    }

    @Test func versionOneWorkspaceWithoutFilePanesDecodes() throws {
        let json = """
            {"id":"ws-a","focusedLeafId":0,"nextLeafId":1,
             "tree":{"type":"leaf","leafId":0,"agentId":"a"}}
            """
        let decoded = try JSONDecoder().decode(PersistedWorkspace.self, from: Data(json.utf8))
        #expect(decoded.filePanes == nil)
        #expect(decoded.nextSurfaceId == nil)
    }
}
