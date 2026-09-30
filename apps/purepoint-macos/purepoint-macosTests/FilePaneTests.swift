import Foundation
import Testing

@testable import PurePoint

@Suite struct FilePaneTests {
    private func twoPaneWorkspace() -> Workspace {
        Workspace(
            id: "ws-a", container: .projectRoot,
            root: .split(
                axis: .vertical, ratio: 0.5,
                first: .leaf(id: 0, agentId: "a"), second: .leaf(id: 1, agentId: nil)),
            focusedLeafId: 1, nextLeafId: 2)
    }

    @Test func givenEmptyLeafSetFilePaneRecordsPath() {
        var ws = twoPaneWorkspace()
        ws.setFilePane(leafId: 1, path: "/tmp/x.md")
        #expect(ws.filePanes[1]?.openPath == "/tmp/x.md")
        #expect(ws.isFilePane(leafId: 1))
    }

    @Test func givenAgentLeafSetFilePaneIsRefused() {
        var ws = twoPaneWorkspace()
        ws.setFilePane(leafId: 0, path: nil)
        #expect(ws.filePanes.isEmpty)
    }

    @Test func givenFilePaneClosedEntryIsRemoved() {
        var ws = twoPaneWorkspace()
        ws.setFilePane(leafId: 1, path: nil)
        _ = ws.closePane(leafId: 1)
        #expect(ws.filePanes.isEmpty)
    }

    @Test func givenAgentBoundToFilePaneLeafEntryIsRemoved() {
        var ws = twoPaneWorkspace()
        ws.setFilePane(leafId: 1, path: nil)
        ws.setAgent("b", forLeafId: 1)
        #expect(ws.filePanes.isEmpty)
    }

    @Test func givenStaleEntryNormalizeDropsIt() {
        var ws = twoPaneWorkspace()
        ws.filePanes[9] = FilePaneConfig(openPath: nil)
        ws.normalize()
        #expect(ws.filePanes.isEmpty)
    }

    @Test func reconcileKeepsFilePaneNextToLiveAgent() {
        var ws = twoPaneWorkspace()
        ws.setFilePane(leafId: 1, path: "/tmp/x.md")
        let result = WorkspaceReconciler.reconcile(
            stored: [ws], live: [LiveAgent(id: "a", container: .projectRoot)])
        #expect(result.count == 1)
        #expect(result[0].filePanes[1]?.openPath == "/tmp/x.md")
    }

    @Test func persistenceRoundTripsFilePanes() throws {
        var ws = twoPaneWorkspace()
        ws.setFilePane(leafId: 1, path: "/tmp/x.md")
        let root = NSTemporaryDirectory() + "pu-filepane-\(UUID().uuidString)"
        defer { try? FileManager.default.removeItem(atPath: root) }
        WorkspacePersistence.save([ws], projectRoot: root)
        let loaded = WorkspacePersistence.load(projectRoot: root)
        #expect(loaded.first?.filePanes[1]?.openPath == "/tmp/x.md")
    }

    @Test func oldDocumentWithoutFilePanesDecodes() throws {
        let json = """
            {"id":"ws-a","focusedLeafId":0,"nextLeafId":1,
             "tree":{"type":"leaf","leafId":0,"agentId":"a"}}
            """
        let decoded = try JSONDecoder().decode(PersistedWorkspace.self, from: Data(json.utf8))
        #expect(decoded.filePanes == nil)
    }
}
