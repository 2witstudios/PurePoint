//
//  purepoint_macosTests.swift
//  purepoint-macosTests
//
//  Created by Jonathan Woodall on 2/28/26.
//

import Testing
import Foundation
@testable import PurePoint

@MainActor
struct GlobalAgentSettingsTests {
    @Test func projectlessSettingsEnsureDaemonBeforeLoadAndSave() async {
        var calls: [String] = []
        let state = AgentConfigState(
            ensureDaemon: { calls.append("ensure") },
            sendRequest: { request in
                switch request {
                case .getGlobalAgentSettings:
                    calls.append("load")
                    return .globalAgentSettingsReport(codexYolo: false)
                case .updateGlobalAgentSettings(let enabled):
                    calls.append("save")
                    return .globalAgentSettingsReport(codexYolo: enabled)
                default:
                    Issue.record("Unexpected project request")
                    return .error(code: "TEST", message: "Unexpected request")
                }
            }
        )
        await state.loadGlobalSettings()
        #expect(state.globalSettingsLoaded)
        await state.updateGlobalSettings(codexYolo: true, projectRoot: nil)
        #expect(state.codexYolo)
        #expect(calls == ["ensure", "load", "ensure", "save"])
    }

    @Test func projectlessSettingsCanRetryDaemonStartupFailure() async {
        var attempts = 0
        var requests = 0
        let state = AgentConfigState(
            ensureDaemon: {
                attempts += 1
                if attempts == 1 { throw NSError(domain: "Startup", code: 1) }
            },
            sendRequest: { _ in
                requests += 1
                return .globalAgentSettingsReport(codexYolo: true)
            }
        )
        await state.loadGlobalSettings()
        #expect(!state.globalSettingsLoaded)
        #expect(state.globalError != nil)
        #expect(requests == 0)
        await state.loadGlobalSettings()
        #expect(state.globalSettingsLoaded)
        #expect(state.globalError == nil)
        #expect(requests == 1)
    }

    @Test func yoloSurvivesEditingModelAndSearch() {
        var config = parseCodexLaunchArgs(["--dangerously-bypass-approvals-and-sandbox", "--no-daemon", "-m", "custom", "--search"])
        #expect(config.yolo)
        config.model = "updated"
        #expect(composeCodexLaunchArgs(config) == ["--dangerously-bypass-approvals-and-sandbox", "--no-daemon", "-m", "updated", "--search"])
        config.yolo = false
        #expect(!composeCodexLaunchArgs(config).contains("--dangerously-bypass-approvals-and-sandbox"))
        #expect(composeCodexLaunchArgs(config).contains("workspace-write"))
    }

    @Test func globalSettingsProtocolRoundTrip() throws {
        let data = try JSONEncoder().encode(DaemonRequest.updateGlobalAgentSettings(codexYolo: true))
        let json = try #require(JSONSerialization.jsonObject(with: data) as? [String: Any])
        #expect(json["type"] as? String == "update_global_agent_settings")
        #expect(json["codex_yolo"] as? Bool == true)
        #expect(json["project_root"] == nil)
        let response = try JSONDecoder().decode(DaemonResponse.self, from: Data(#"{"type":"global_agent_settings_report","codex_yolo":true}"#.utf8))
        guard case .globalAgentSettingsReport(let enabled) = response else {
            Issue.record("Expected global settings response")
            return
        }
        #expect(enabled)
    }
}

// MARK: - PaneSplitNode Tests

struct PaneSplitNodeTests {

    // MARK: Leaf creation

    @Test func leafNodeStoresId() {
        let node = PaneSplitNode.leaf(id: 1)
        #expect(node.allLeafIds == [1])
        #expect(node.leafCount == 1)
    }


    @Test func leafRowCountIsOne() {
        let node = PaneSplitNode.leaf(id: 5)
        #expect(node.rowCount == 1)
    }

    // MARK: Splitting

    @Test func splitHorizontallyCreatesTwo() {
        let leaf = PaneSplitNode.leaf(id: 0)
        var nextId = 1
        let split = leaf.splittingLeaf(id: 0, axis: .horizontal, nextId: &nextId)
        #expect(split.leafCount == 2)
        #expect(split.allLeafIds == [0, 1])
        #expect(nextId == 2)
    }

    @Test func splitVerticallyCreatesTwo() {
        let leaf = PaneSplitNode.leaf(id: 0)
        var nextId = 1
        let split = leaf.splittingLeaf(id: 0, axis: .vertical, nextId: &nextId)
        #expect(split.leafCount == 2)
        #expect(split.allLeafIds == [0, 1])
    }

    @Test func splitKeepsOriginalLeafFirst() {
        let leaf = PaneSplitNode.leaf(id: 0)
        var nextId = 1
        let split = leaf.splittingLeaf(id: 0, axis: .horizontal, nextId: &nextId)
        #expect(split == .split(axis: .horizontal, ratio: 0.5, first: .leaf(id: 0), second: .leaf(id: 1)))
    }

    @Test func splitNonexistentIdIsNoOp() {
        let leaf = PaneSplitNode.leaf(id: 0)
        var nextId = 1
        let result = leaf.splittingLeaf(id: 99, axis: .horizontal, nextId: &nextId)
        #expect(result == leaf)
        #expect(nextId == 1)
    }

    @Test func splitNestedLeaf() {
        var nextId = 2
        let tree = PaneSplitNode.split(
            axis: .horizontal, ratio: 0.5,
            first: .leaf(id: 0),
            second: .leaf(id: 1)
        )
        let result = tree.splittingLeaf(id: 1, axis: .vertical, nextId: &nextId)
        #expect(result.leafCount == 3)
        #expect(result.allLeafIds == [0, 1, 2])
    }

    @Test func splitNestedLeafPreservesCustomRatio() {
        var nextId = 2
        let tree = PaneSplitNode.split(
            axis: .horizontal, ratio: 0.5,
            first: .leaf(id: 0),
            second: .leaf(id: 1)
        )
        let result = tree.splittingLeaf(id: 1, axis: .vertical, ratio: 0.7, nextId: &nextId)
        // The inner split created for leaf 1 should have ratio 0.7
        let expected = PaneSplitNode.split(
            axis: .horizontal, ratio: 0.5,
            first: .leaf(id: 0),
            second: .split(
                axis: .vertical, ratio: 0.7,
                first: .leaf(id: 1),
                second: .leaf(id: 2)
            )
        )
        #expect(result == expected)
    }

    // MARK: Row count / canSplit

    @Test func horizontalSplitCountsRows() {
        let tree = PaneSplitNode.split(
            axis: .horizontal, ratio: 0.5,
            first: .leaf(id: 0),
            second: .leaf(id: 1)
        )
        #expect(tree.rowCount == 2)
    }

    @Test func verticalSplitRowCountIsOne() {
        let tree = PaneSplitNode.split(
            axis: .vertical, ratio: 0.5,
            first: .leaf(id: 0),
            second: .leaf(id: 1)
        )
        #expect(tree.rowCount == 1)
    }

    @Test func canSplitUnderSixLeaves() {
        let leaf = PaneSplitNode.leaf(id: 0)
        #expect(leaf.canSplit(axis: .horizontal) == true)
    }

    @Test func canSplitAtFiveLeaves() {
        // Build a tree with 5 leaves — should still allow one more split
        var nextId = 1
        var tree = PaneSplitNode.leaf(id: 0)
        for _ in 0..<4 {
            let target = tree.allLeafIds.last!
            tree = tree.splittingLeaf(id: target, axis: .vertical, nextId: &nextId)
        }
        #expect(tree.leafCount == 5)
        #expect(tree.canSplit(axis: .horizontal) == true)
    }

    @Test func cannotSplitAtSixLeaves() {
        // Build a tree with 6 leaves
        var nextId = 1
        var tree = PaneSplitNode.leaf(id: 0)
        for _ in 0..<5 {
            let target = tree.allLeafIds.last!
            tree = tree.splittingLeaf(id: target, axis: .vertical, nextId: &nextId)
        }
        #expect(tree.leafCount == 6)
        #expect(tree.canSplit(axis: .vertical) == false)
    }

    // MARK: Removing / closing

    @Test func removeSingleLeafReturnsNil() {
        let leaf = PaneSplitNode.leaf(id: 0)
        #expect(leaf.removingLeaf(id: 0) == nil)
    }

    @Test func removeNonexistentLeafIsNoOp() {
        let leaf = PaneSplitNode.leaf(id: 0)
        #expect(leaf.removingLeaf(id: 99) == leaf)
    }

    @Test func removeLeafCollapsesParent() {
        let tree = PaneSplitNode.split(
            axis: .horizontal, ratio: 0.5,
            first: .leaf(id: 0),
            second: .leaf(id: 1)
        )
        let result = tree.removingLeaf(id: 0)
        #expect(result == .leaf(id: 1))
    }

    @Test func removeDeeplyNestedLeaf() {
        let tree = PaneSplitNode.split(
            axis: .vertical, ratio: 0.5,
            first: .leaf(id: 0),
            second: .split(
                axis: .horizontal, ratio: 0.5,
                first: .leaf(id: 1),
                second: .leaf(id: 2)
            )
        )
        let result = tree.removingLeaf(id: 1)!
        #expect(result.leafCount == 2)
        #expect(result.allLeafIds == [0, 2])
    }

    // MARK: Finding / queries




    // MARK: Setting agent



    // MARK: Setting ratio

    @Test func settingRatioOnSplit() {
        let tree = PaneSplitNode.split(
            axis: .vertical, ratio: 0.5,
            first: .leaf(id: 0),
            second: .leaf(id: 1)
        )
        let updated = tree.settingRatio(0.7, forSplitIdentifiedByFirstLeaf: 0)
        let expected = PaneSplitNode.split(
            axis: .vertical, ratio: 0.7,
            first: .leaf(id: 0),
            second: .leaf(id: 1)
        )
        #expect(updated == expected)
    }

    // MARK: Spatial navigation

    @Test func findAdjacentLeafForward() {
        let tree = PaneSplitNode.split(
            axis: .vertical, ratio: 0.5,
            first: .leaf(id: 0),
            second: .leaf(id: 1)
        )
        #expect(tree.findAdjacentLeaf(from: 0, axis: .vertical, forward: true) == 1)
    }

    @Test func findAdjacentLeafBackward() {
        let tree = PaneSplitNode.split(
            axis: .vertical, ratio: 0.5,
            first: .leaf(id: 0),
            second: .leaf(id: 1)
        )
        #expect(tree.findAdjacentLeaf(from: 1, axis: .vertical, forward: false) == 0)
    }

    @Test func findAdjacentLeafNoNeighbor() {
        let tree = PaneSplitNode.split(
            axis: .vertical, ratio: 0.5,
            first: .leaf(id: 0),
            second: .leaf(id: 1)
        )
        // No horizontal neighbor exists
        #expect(tree.findAdjacentLeaf(from: 0, axis: .horizontal, forward: true) == nil)
    }

    @Test func siblingLeafId() {
        let tree = PaneSplitNode.split(
            axis: .vertical, ratio: 0.5,
            first: .leaf(id: 0),
            second: .leaf(id: 1)
        )
        #expect(tree.siblingLeafId(of: 0) == 1)
        #expect(tree.siblingLeafId(of: 1) == 0)
    }

    // MARK: Equatable

    @Test func equalityForLeaves() {
        let a = PaneSplitNode.leaf(id: 0)
        let b = PaneSplitNode.leaf(id: 0)
        let c = PaneSplitNode.leaf(id: 1)
        #expect(a == b)
        #expect(a != c)
    }
}

// MARK: - GridLayoutPersistence Round-Trip Tests

struct GridLayoutPersistenceTests {

    private func roundTrip(_ root: PaneSplitNode, panes: [Int: Pane]) throws -> (PaneSplitNode, [Int: Pane], Int) {
        let data = try JSONEncoder().encode(WorkspacePersistence.layoutNode(root, panes: panes))
        let decoded = try JSONDecoder().decode(GridLayoutNode.self, from: data)
        var restoredPanes: [Int: Pane] = [:]
        var nextLeafId = 0
        var nextSurfaceId = 0
        let restored = WorkspacePersistence.tree(
            from: decoded, panes: &restoredPanes, nextLeafId: &nextLeafId, nextSurfaceId: &nextSurfaceId)
        return (restored, restoredPanes, nextSurfaceId)
    }

    private func pane(_ tabs: [(Int, SurfaceContent)], active: Int) -> Pane {
        Pane(tabs: tabs.map { Surface(id: $0.0, content: $0.1) }, activeTabId: active)
    }

    @Test func roundTripSingleLeafKeepsTabsAndActiveTab() throws {
        let panes = [0: pane([(0, .agent("agent-1")), (1, .file(path: "/tmp/a.md")), (2, .empty)], active: 1)]
        let (restored, restoredPanes, nextSurfaceId) = try roundTrip(.leaf(id: 0), panes: panes)
        #expect(restored == .leaf(id: 0))
        #expect(restoredPanes == panes)
        #expect(nextSurfaceId == 3)
    }

    @Test func roundTripSplitTreePreservesGeometryAndRatios() throws {
        let original = PaneSplitNode.split(
            axis: .vertical, ratio: 0.6,
            first: .leaf(id: 0),
            second: .split(axis: .horizontal, ratio: 0.4, first: .leaf(id: 1), second: .leaf(id: 2))
        )
        let panes = [
            0: pane([(0, .agent("a1"))], active: 0),
            1: pane([(1, .agent("a2")), (3, .agent("a3"))], active: 3),
            2: pane([(2, .empty)], active: 2),
        ]
        let (restored, restoredPanes, _) = try roundTrip(original, panes: panes)
        #expect(restored == original)
        #expect(restoredPanes == panes)
    }

    @Test func degradedSplitFallsBackToLeafWithEmptyTab() {
        // A split node with missing axis/ratio/children should degrade to a single leaf
        let degraded = GridLayoutNode(type: .split, leafId: nil, axis: nil, ratio: nil, first: nil, second: nil)
        var panes: [Int: Pane] = [:]
        var nextLeafId = 0
        var nextSurfaceId = 0
        let result = WorkspacePersistence.tree(
            from: degraded, panes: &panes, nextLeafId: &nextLeafId, nextSurfaceId: &nextSurfaceId)
        #expect(result.leafCount == 1)
        #expect(nextLeafId == 1)
        #expect(panes[0]?.tabs.map(\.content) == [.empty])
    }

    @Test func unknownSurfaceKindLoadsAsEmptyTab() throws {
        let json = """
            {"type":"leaf","leafId":0,"activeTabId":4,"tabs":[{"id":4,"kind":"browser"}]}
            """
        let node = try JSONDecoder().decode(GridLayoutNode.self, from: Data(json.utf8))
        var panes: [Int: Pane] = [:]
        var nextLeafId = 0
        var nextSurfaceId = 0
        _ = WorkspacePersistence.tree(from: node, panes: &panes, nextLeafId: &nextLeafId, nextSurfaceId: &nextSurfaceId)
        #expect(panes[0] == Pane(tabs: [Surface(id: 4, content: .empty)], activeTabId: 4))
    }

    @Test func roundTripPersistedWorkspace() throws {
        let tree = PaneSplitNode.split(axis: .horizontal, ratio: 0.5, first: .leaf(id: 0), second: .leaf(id: 1))
        let panes = [0: pane([(0, .agent("owner"))], active: 0), 1: pane([(1, .agent("worker"))], active: 1)]
        let persisted = PersistedWorkspaceDocument(
            version: PersistedWorkspaceDocument.currentVersion,
            workspaces: [
                PersistedWorkspace(
                    id: "ws-owner", worktreeId: nil, focusedLeafId: 1, nextLeafId: 2, nextSurfaceId: 2,
                    tree: WorkspacePersistence.layoutNode(tree, panes: panes), filePanes: nil)
            ])
        let data = try JSONEncoder().encode(persisted)
        let decoded = try JSONDecoder().decode(PersistedWorkspaceDocument.self, from: data)
        let workspace = try #require(decoded.workspaces.first)
        #expect(decoded.version == 3)
        #expect(workspace.id == "ws-owner")
        #expect(workspace.focusedLeafId == 1)
        #expect(workspace.nextSurfaceId == 2)
        #expect(workspace.tree.first?.tabs?.first?.agentId == "owner")
        #expect(workspace.tree.second?.tabs?.first?.agentId == "worker")
    }
}
