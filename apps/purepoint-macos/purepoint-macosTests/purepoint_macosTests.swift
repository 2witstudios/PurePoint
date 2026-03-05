//
//  purepoint_macosTests.swift
//  purepoint-macosTests
//
//  Created by Jonathan Woodall on 2/28/26.
//

import Testing
import Foundation
@testable import purepoint_macos

// MARK: - PaneSplitNode Tests

struct PaneSplitNodeTests {

    // MARK: Leaf creation

    @Test func leafNodeStoresIdAndAgent() {
        let node = PaneSplitNode.leaf(id: 1, agentId: "agent-a")
        #expect(node.allLeafIds == [1])
        #expect(node.leafCount == 1)
        #expect(node.agentId(forLeafId: 1) == "agent-a")
    }

    @Test func leafNodeWithNilAgent() {
        let node = PaneSplitNode.leaf(id: 0, agentId: nil)
        #expect(node.agentId(forLeafId: 0) == nil)
        #expect(node.leafCount == 1)
    }

    @Test func leafRowCountIsOne() {
        let node = PaneSplitNode.leaf(id: 5, agentId: nil)
        #expect(node.rowCount == 1)
    }

    // MARK: Splitting

    @Test func splitHorizontallyCreatesTwo() {
        let leaf = PaneSplitNode.leaf(id: 0, agentId: "a1")
        var nextId = 1
        let split = leaf.splittingLeaf(id: 0, axis: .horizontal, nextId: &nextId)
        #expect(split.leafCount == 2)
        #expect(split.allLeafIds == [0, 1])
        #expect(nextId == 2)
    }

    @Test func splitVerticallyCreatesTwo() {
        let leaf = PaneSplitNode.leaf(id: 0, agentId: nil)
        var nextId = 1
        let split = leaf.splittingLeaf(id: 0, axis: .vertical, nextId: &nextId)
        #expect(split.leafCount == 2)
        #expect(split.allLeafIds == [0, 1])
    }

    @Test func splitPreservesOriginalAgent() {
        let leaf = PaneSplitNode.leaf(id: 0, agentId: "original")
        var nextId = 1
        let split = leaf.splittingLeaf(id: 0, axis: .horizontal, nextId: &nextId)
        #expect(split.agentId(forLeafId: 0) == "original")
        #expect(split.agentId(forLeafId: 1) == nil)
    }

    @Test func splitNonexistentIdIsNoOp() {
        let leaf = PaneSplitNode.leaf(id: 0, agentId: nil)
        var nextId = 1
        let result = leaf.splittingLeaf(id: 99, axis: .horizontal, nextId: &nextId)
        #expect(result == leaf)
        #expect(nextId == 1)
    }

    @Test func splitNestedLeaf() {
        var nextId = 2
        let tree = PaneSplitNode.split(
            axis: .horizontal, ratio: 0.5,
            first: .leaf(id: 0, agentId: nil),
            second: .leaf(id: 1, agentId: nil)
        )
        let result = tree.splittingLeaf(id: 1, axis: .vertical, nextId: &nextId)
        #expect(result.leafCount == 3)
        #expect(result.allLeafIds == [0, 1, 2])
    }

    @Test func splitNestedLeafPreservesCustomRatio() {
        var nextId = 2
        let tree = PaneSplitNode.split(
            axis: .horizontal, ratio: 0.5,
            first: .leaf(id: 0, agentId: nil),
            second: .leaf(id: 1, agentId: nil)
        )
        let result = tree.splittingLeaf(id: 1, axis: .vertical, ratio: 0.7, nextId: &nextId)
        // The inner split created for leaf 1 should have ratio 0.7
        let expected = PaneSplitNode.split(
            axis: .horizontal, ratio: 0.5,
            first: .leaf(id: 0, agentId: nil),
            second: .split(
                axis: .vertical, ratio: 0.7,
                first: .leaf(id: 1, agentId: nil),
                second: .leaf(id: 2, agentId: nil)
            )
        )
        #expect(result == expected)
    }

    // MARK: Row count / canSplit

    @Test func horizontalSplitCountsRows() {
        let tree = PaneSplitNode.split(
            axis: .horizontal, ratio: 0.5,
            first: .leaf(id: 0, agentId: nil),
            second: .leaf(id: 1, agentId: nil)
        )
        #expect(tree.rowCount == 2)
    }

    @Test func verticalSplitRowCountIsOne() {
        let tree = PaneSplitNode.split(
            axis: .vertical, ratio: 0.5,
            first: .leaf(id: 0, agentId: nil),
            second: .leaf(id: 1, agentId: nil)
        )
        #expect(tree.rowCount == 1)
    }

    @Test func canSplitUnderSixLeaves() {
        let leaf = PaneSplitNode.leaf(id: 0, agentId: nil)
        #expect(leaf.canSplit(axis: .horizontal) == true)
    }

    @Test func canSplitAtFiveLeaves() {
        // Build a tree with 5 leaves — should still allow one more split
        var nextId = 1
        var tree = PaneSplitNode.leaf(id: 0, agentId: nil)
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
        var tree = PaneSplitNode.leaf(id: 0, agentId: nil)
        for _ in 0..<5 {
            let target = tree.allLeafIds.last!
            tree = tree.splittingLeaf(id: target, axis: .vertical, nextId: &nextId)
        }
        #expect(tree.leafCount == 6)
        #expect(tree.canSplit(axis: .vertical) == false)
    }

    // MARK: Removing / closing

    @Test func removeSingleLeafReturnsNil() {
        let leaf = PaneSplitNode.leaf(id: 0, agentId: nil)
        #expect(leaf.removingLeaf(id: 0) == nil)
    }

    @Test func removeNonexistentLeafIsNoOp() {
        let leaf = PaneSplitNode.leaf(id: 0, agentId: nil)
        #expect(leaf.removingLeaf(id: 99) == leaf)
    }

    @Test func removeLeafCollapsesParent() {
        let tree = PaneSplitNode.split(
            axis: .horizontal, ratio: 0.5,
            first: .leaf(id: 0, agentId: "a"),
            second: .leaf(id: 1, agentId: "b")
        )
        let result = tree.removingLeaf(id: 0)
        #expect(result == .leaf(id: 1, agentId: "b"))
    }

    @Test func removeDeeplyNestedLeaf() {
        let tree = PaneSplitNode.split(
            axis: .vertical, ratio: 0.5,
            first: .leaf(id: 0, agentId: nil),
            second: .split(
                axis: .horizontal, ratio: 0.5,
                first: .leaf(id: 1, agentId: nil),
                second: .leaf(id: 2, agentId: nil)
            )
        )
        let result = tree.removingLeaf(id: 1)!
        #expect(result.leafCount == 2)
        #expect(result.allLeafIds == [0, 2])
    }

    // MARK: Finding / queries

    @Test func containsAgentFindsIt() {
        let tree = PaneSplitNode.split(
            axis: .vertical, ratio: 0.5,
            first: .leaf(id: 0, agentId: "agent-x"),
            second: .leaf(id: 1, agentId: "agent-y")
        )
        #expect(tree.containsAgent("agent-x") == true)
        #expect(tree.containsAgent("agent-y") == true)
        #expect(tree.containsAgent("agent-z") == false)
    }

    @Test func leafIdForAgentId() {
        let tree = PaneSplitNode.split(
            axis: .horizontal, ratio: 0.5,
            first: .leaf(id: 0, agentId: "a1"),
            second: .leaf(id: 1, agentId: "a2")
        )
        #expect(tree.leafId(forAgentId: "a1") == 0)
        #expect(tree.leafId(forAgentId: "a2") == 1)
        #expect(tree.leafId(forAgentId: "missing") == nil)
    }

    @Test func agentIdForMissingLeafReturnsNil() {
        let leaf = PaneSplitNode.leaf(id: 0, agentId: "a")
        #expect(leaf.agentId(forLeafId: 99) == nil)
    }

    // MARK: Setting agent

    @Test func settingAgentOnLeaf() {
        let leaf = PaneSplitNode.leaf(id: 0, agentId: nil)
        let updated = leaf.settingAgent("new-agent", forLeafId: 0)
        #expect(updated.agentId(forLeafId: 0) == "new-agent")
    }

    @Test func settingAgentOnWrongLeafIsNoOp() {
        let leaf = PaneSplitNode.leaf(id: 0, agentId: "original")
        let updated = leaf.settingAgent("new", forLeafId: 99)
        #expect(updated == leaf)
    }

    // MARK: Setting ratio

    @Test func settingRatioOnSplit() {
        let tree = PaneSplitNode.split(
            axis: .vertical, ratio: 0.5,
            first: .leaf(id: 0, agentId: nil),
            second: .leaf(id: 1, agentId: nil)
        )
        let updated = tree.settingRatio(0.7, forSplitIdentifiedByFirstLeaf: 0)
        let expected = PaneSplitNode.split(
            axis: .vertical, ratio: 0.7,
            first: .leaf(id: 0, agentId: nil),
            second: .leaf(id: 1, agentId: nil)
        )
        #expect(updated == expected)
    }

    // MARK: Spatial navigation

    @Test func findAdjacentLeafForward() {
        let tree = PaneSplitNode.split(
            axis: .vertical, ratio: 0.5,
            first: .leaf(id: 0, agentId: nil),
            second: .leaf(id: 1, agentId: nil)
        )
        #expect(tree.findAdjacentLeaf(from: 0, axis: .vertical, forward: true) == 1)
    }

    @Test func findAdjacentLeafBackward() {
        let tree = PaneSplitNode.split(
            axis: .vertical, ratio: 0.5,
            first: .leaf(id: 0, agentId: nil),
            second: .leaf(id: 1, agentId: nil)
        )
        #expect(tree.findAdjacentLeaf(from: 1, axis: .vertical, forward: false) == 0)
    }

    @Test func findAdjacentLeafNoNeighbor() {
        let tree = PaneSplitNode.split(
            axis: .vertical, ratio: 0.5,
            first: .leaf(id: 0, agentId: nil),
            second: .leaf(id: 1, agentId: nil)
        )
        // No horizontal neighbor exists
        #expect(tree.findAdjacentLeaf(from: 0, axis: .horizontal, forward: true) == nil)
    }

    @Test func siblingLeafId() {
        let tree = PaneSplitNode.split(
            axis: .vertical, ratio: 0.5,
            first: .leaf(id: 0, agentId: nil),
            second: .leaf(id: 1, agentId: nil)
        )
        #expect(tree.siblingLeafId(of: 0) == 1)
        #expect(tree.siblingLeafId(of: 1) == 0)
    }

    // MARK: Equatable

    @Test func equalityForLeaves() {
        let a = PaneSplitNode.leaf(id: 0, agentId: "x")
        let b = PaneSplitNode.leaf(id: 0, agentId: "x")
        let c = PaneSplitNode.leaf(id: 1, agentId: "x")
        #expect(a == b)
        #expect(a != c)
    }
}

// MARK: - GridLayoutPersistence Round-Trip Tests

struct GridLayoutPersistenceTests {

    @Test func roundTripSingleLeaf() throws {
        let original = PaneSplitNode.leaf(id: 0, agentId: "agent-1")
        let layoutNode = original.toLayoutNode()
        let data = try JSONEncoder().encode(layoutNode)
        let decoded = try JSONDecoder().decode(GridLayoutNode.self, from: data)
        var nextId = 0
        let restored = PaneSplitNode.fromLayoutNode(decoded, nextId: &nextId)
        // Leaf IDs are reassigned on restore, so check structure and agent
        #expect(restored.leafCount == 1)
        #expect(restored.agentId(forLeafId: 0) == "agent-1")
    }

    @Test func roundTripSplitTree() throws {
        let original = PaneSplitNode.split(
            axis: .vertical, ratio: 0.6,
            first: .leaf(id: 0, agentId: "a1"),
            second: .split(
                axis: .horizontal, ratio: 0.4,
                first: .leaf(id: 1, agentId: "a2"),
                second: .leaf(id: 2, agentId: nil)
            )
        )
        let layoutNode = original.toLayoutNode()
        let data = try JSONEncoder().encode(layoutNode)
        let decoded = try JSONDecoder().decode(GridLayoutNode.self, from: data)
        var nextId = 0
        let restored = PaneSplitNode.fromLayoutNode(decoded, nextId: &nextId)
        #expect(restored.leafCount == 3)
        #expect(restored.containsAgent("a1"))
        #expect(restored.containsAgent("a2"))
        #expect(nextId == 3)
    }

    @Test func roundTripPreservesRatios() throws {
        let original = PaneSplitNode.split(
            axis: .vertical, ratio: 0.6,
            first: .leaf(id: 0, agentId: "a1"),
            second: .split(
                axis: .horizontal, ratio: 0.4,
                first: .leaf(id: 1, agentId: "a2"),
                second: .leaf(id: 2, agentId: nil)
            )
        )
        let layoutNode = original.toLayoutNode()
        let data = try JSONEncoder().encode(layoutNode)
        let decoded = try JSONDecoder().decode(GridLayoutNode.self, from: data)
        var nextId = 0
        let restored = PaneSplitNode.fromLayoutNode(decoded, nextId: &nextId)
        // Compare layout nodes to verify ratios survive the round trip
        let restoredLayout = restored.toLayoutNode()
        let originalLayout = original.toLayoutNode()
        #expect(restoredLayout.ratio == originalLayout.ratio)
        #expect(restoredLayout.first?.ratio == originalLayout.first?.ratio)
        #expect(restoredLayout.second?.ratio == originalLayout.second?.ratio)
    }

    @Test func fromLayoutNodeDegradedSplitFallsBackToLeaf() {
        // A split node with missing axis/ratio/children should degrade to a single leaf
        let degraded = GridLayoutNode(type: .split, agentId: nil, axis: nil, ratio: nil, first: nil, second: nil)
        var nextId = 0
        let result = PaneSplitNode.fromLayoutNode(degraded, nextId: &nextId)
        #expect(result.leafCount == 1)
        #expect(nextId == 1)
    }

    @Test func roundTripPersistedGridLayout() throws {
        let tree = PaneSplitNode.split(
            axis: .horizontal, ratio: 0.5,
            first: .leaf(id: 0, agentId: "owner"),
            second: .leaf(id: 1, agentId: "worker")
        )
        let persisted = PersistedGridLayout(ownerAgentId: "owner", tree: tree.toLayoutNode())
        let data = try JSONEncoder().encode(persisted)
        let decoded = try JSONDecoder().decode(PersistedGridLayout.self, from: data)
        #expect(decoded.ownerAgentId == "owner")
        var nextId = 0
        let restored = PaneSplitNode.fromLayoutNode(decoded.tree, nextId: &nextId)
        #expect(restored.leafCount == 2)
        #expect(restored.containsAgent("owner"))
        #expect(restored.containsAgent("worker"))
    }
}
