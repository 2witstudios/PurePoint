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
                    return .globalAgentSettingsReport(agentStatusTracking: true)
                case .updateGlobalAgentSettings(let tracking, _):
                    calls.append("save")
                    return .globalAgentSettingsReport(agentStatusTracking: tracking ?? true)
                default:
                    Issue.record("Unexpected project request")
                    return .error(code: "TEST", message: "Unexpected request")
                }
            }
        )
        await state.loadGlobalSettings()
        #expect(state.globalSettingsLoaded)
        await state.updateAgentStatusTracking(false)
        #expect(!state.agentStatusTracking)
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
                return .globalAgentSettingsReport(agentStatusTracking: true)
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

    @Test func projectLaunchArgsAreSentAsOneString() throws {
        let data = try JSONEncoder().encode(DaemonRequest.updateAgentConfig(
            projectRoot: "/p", agentName: "claude", launchArgs: "--model opus --append-system-prompt 'be terse'"))
        let json = try #require(JSONSerialization.jsonObject(with: data) as? [String: Any])
        #expect(json["launch_args"] as? String == "--model opus --append-system-prompt 'be terse'")
        let cleared = try JSONEncoder().encode(DaemonRequest.updateAgentConfig(projectRoot: "/p", agentName: "claude", launchArgs: nil))
        let clearedJson = try #require(JSONSerialization.jsonObject(with: cleared) as? [String: Any])
        #expect(clearedJson["launch_args"] is NSNull, "null clears the project value")
    }

    @Test func machineLaunchArgsSaveSendsOnlyThatAgent() async throws {
        let data = try JSONEncoder().encode(DaemonRequest.updateGlobalAgentSettings(launchArgs: ["claude": "--dangerously-skip-permissions"]))
        let json = try #require(JSONSerialization.jsonObject(with: data) as? [String: Any])
        #expect(json["launch_args"] as? [String: String] == ["claude": "--dangerously-skip-permissions"])
        #expect(json["codex_yolo"] == nil)
        let state = AgentConfigState(
            ensureDaemon: {},
            sendRequest: { request in
                guard case .updateGlobalAgentSettings(_, let launchArgs) = request else {
                    return .error(code: "TEST", message: "Unexpected request")
                }
                return .globalAgentSettingsReport(agentStatusTracking: true, launchArgs: launchArgs ?? [:])
            }
        )
        await state.updateMachineLaunchArgs(agentName: "claude", launchArgs: "--model opus", projectRoot: nil)
        #expect(state.machineLaunchArgs == ["claude": "--model opus"])
    }

    @Test func oldDaemonMachineLaunchArgsSaveReportsUpdateNeeded() async {
        let state = AgentConfigState(
            ensureDaemon: {},
            sendRequest: { request in
                switch request {
                case .getGlobalAgentSettings:
                    return .globalAgentSettingsReport(agentStatusTracking: true, launchArgs: ["claude": "--model opus"])
                default:
                    // A protocol-v10 daemon ignores machine launch args and omits them from its report.
                    return .globalAgentSettingsReport(agentStatusTracking: true)
                }
            }
        )
        await state.loadGlobalSettings()
        await state.updateMachineLaunchArgs(agentName: "claude", launchArgs: "--model sonnet", projectRoot: nil)
        #expect(state.globalError?.contains("Update the PurePoint daemon") == true)
        #expect(state.machineLaunchArgs == ["claude": "--model opus"], "An old daemon's report must not wipe the field")
    }

    @Test func oldDaemonConfigReportReportsUpdateNeeded() async {
        let oldAgent = AgentConfigPayload(name: "claude", command: "claude", launchArgs: nil, resolvedLaunchArgs: [], interactive: true)
        let state = AgentConfigState(
            ensureDaemon: {},
            sendRequest: { _ in .configReport(defaultAgent: "claude", agents: [oldAgent]) }
        )
        await state.load(projectRoot: "/p")
        #expect(state.error?.contains("Update the PurePoint daemon") == true)
        await state.updateLaunchArgs(projectRoot: "/p", agentName: "claude", launchArgs: "--model opus")
        #expect(state.error?.contains("Update the PurePoint daemon") == true)
    }

    @Test func configReportDecodesLaunchArgStrings() throws {
        let json = #"{"type":"config_report","default_agent":"claude","agents":[{"name":"claude","command":"claude","launch_args":null,"resolved_launch_args":["--model","opus"],"interactive":true,"machine_launch_args":"--model opus","resolved_command":"claude --model opus"}]}"#
        let response = try JSONDecoder().decode(DaemonResponse.self, from: Data(json.utf8))
        guard case .configReport(_, let agents) = response, let claude = agents.first else {
            Issue.record("Expected config report")
            return
        }
        #expect(claude.projectLaunchArgs == nil)
        #expect(claude.machineLaunchArgs == "--model opus")
        #expect(claude.resolvedCommand == "claude --model opus")
    }

    @Test func globalSettingsProtocolRoundTrip() throws {
        let data = try JSONEncoder().encode(DaemonRequest.updateGlobalAgentSettings(agentStatusTracking: true))
        let json = try #require(JSONSerialization.jsonObject(with: data) as? [String: Any])
        #expect(json["type"] as? String == "update_global_agent_settings")
        #expect(json["codex_yolo"] == nil)
        #expect(json["project_root"] == nil)
        #expect(json["launch_args"] == nil)
        // Daemons from before Codex YOLO was removed still send `codex_yolo`; it is ignored.
        let response = try JSONDecoder().decode(DaemonResponse.self, from: Data(#"{"type":"global_agent_settings_report","codex_yolo":true}"#.utf8))
        guard case .globalAgentSettingsReport(let tracking, let launchArgs) = response else {
            Issue.record("Expected global settings response")
            return
        }
        #expect(tracking, "Older daemons always track status")
        #expect(launchArgs == nil, "Older daemons have no machine launch args")
    }

    @Test func statusTrackingUpdateSendsOnlyItsOwnField() async throws {
        let data = try JSONEncoder().encode(DaemonRequest.updateGlobalAgentSettings(agentStatusTracking: false))
        let json = try #require(JSONSerialization.jsonObject(with: data) as? [String: Any])
        #expect(json["agent_status_tracking"] as? Bool == false)
        #expect(json["codex_yolo"] == nil)
        let state = AgentConfigState(
            ensureDaemon: {},
            sendRequest: { _ in .globalAgentSettingsReport(agentStatusTracking: false) }
        )
        await state.updateAgentStatusTracking(false)
        #expect(!state.agentStatusTracking)
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

    // MARK: Top-trailing leaf

    @Test func singleLeafIsTopTrailing() {
        #expect(PaneSplitNode.leaf(id: 3).topTrailingLeafId == 3)
    }

    @Test func rightPaneOfAVerticalSplitIsTopTrailing() {
        let tree = PaneSplitNode.split(axis: .vertical, ratio: 0.5, first: .leaf(id: 0), second: .leaf(id: 1))
        #expect(tree.topTrailingLeafId == 1)
    }

    @Test func topPaneOfAHorizontalSplitIsTopTrailing() {
        let tree = PaneSplitNode.split(axis: .horizontal, ratio: 0.5, first: .leaf(id: 0), second: .leaf(id: 1))
        #expect(tree.topTrailingLeafId == 0)
    }

    @Test func nestedGridResolvesTheTopRightPane() {
        // Two columns; the right column is split into a top (2) and bottom (1) pane.
        let tree = PaneSplitNode.split(
            axis: .vertical, ratio: 0.5,
            first: .leaf(id: 0),
            second: .split(axis: .horizontal, ratio: 0.5, first: .leaf(id: 2), second: .leaf(id: 1))
        )
        #expect(tree.topTrailingLeafId == 2)
    }

    // MARK: Top-leading leaf (the pane under the traffic lights while the sidebar is hidden)

    @Test func leftPaneOfAVerticalSplitIsTopLeading() {
        let tree = PaneSplitNode.split(axis: .vertical, ratio: 0.5, first: .leaf(id: 0), second: .leaf(id: 1))
        #expect(tree.firstLeafId == 0)
    }

    @Test func topPaneOfAHorizontalSplitIsTopLeading() {
        let tree = PaneSplitNode.split(axis: .horizontal, ratio: 0.5, first: .leaf(id: 1), second: .leaf(id: 0))
        #expect(tree.firstLeafId == 1)
    }

    @Test func nestedGridResolvesTheTopLeftPane() {
        // Two rows; the top row is split into a left (3) and right (2) pane.
        let tree = PaneSplitNode.split(
            axis: .horizontal, ratio: 0.5,
            first: .split(axis: .vertical, ratio: 0.5, first: .leaf(id: 3), second: .leaf(id: 2)),
            second: .leaf(id: 0)
        )
        #expect(tree.firstLeafId == 3)
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
        #expect(decoded.version == PersistedWorkspaceDocument.currentVersion)
        #expect(workspace.id == "ws-owner")
        #expect(workspace.focusedLeafId == 1)
        #expect(workspace.nextSurfaceId == 2)
        #expect(workspace.tree.first?.tabs?.first?.agentId == "owner")
        #expect(workspace.tree.second?.tabs?.first?.agentId == "worker")
    }
}
