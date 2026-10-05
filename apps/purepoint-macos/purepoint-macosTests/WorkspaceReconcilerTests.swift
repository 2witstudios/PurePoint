import Foundation
import Testing

@testable import PurePoint

private func makeAgent(_ id: String) -> AgentModel {
    AgentModel(id: id, name: id, agentType: "claude", status: .running, prompt: "", startedAt: "")
}

private func makeWorktree(_ id: String, agents: [String]) -> WorktreeModel {
    WorktreeModel(
        id: id, name: id, path: "/tmp/\(id)", branch: "pu/\(id)", status: "active",
        agents: agents.map(makeAgent))
}

private func live(root: [String] = [], worktrees: [(String, [String])] = []) -> [LiveAgent] {
    WorkspaceReconciler.liveAgents(
        rootAgents: root.map(makeAgent),
        worktrees: worktrees.map { makeWorktree($0.0, agents: $0.1) }
    )
}

/// A three-pane workspace: ag-a split right into ag-b, then that split down into ag-c.
private func makeGroupedWorkspace() -> Workspace {
    var workspace = Workspace.adopting(agentId: "ag-a", container: .projectRoot)
    workspace.split(leafId: 0, axis: .vertical, content: .agent("ag-b"))
    workspace.split(leafId: workspace.focusedLeafId, axis: .horizontal, content: .agent("ag-c"))
    return workspace
}

private func withTempProject(_ body: (String) throws -> Void) rethrows {
    let root = NSTemporaryDirectory() + "pp-workspace-tests-" + UUID().uuidString
    try? FileManager.default.createDirectory(
        atPath: root + "/.pu", withIntermediateDirectories: true)
    defer { try? FileManager.default.removeItem(atPath: root) }
    try body(root)
}

@Suite
struct WorkspaceReconcilerTests {

    // MARK: - The invariant

    /// The property the whole workspace model exists to guarantee. Every test below leans
    /// on this: if it holds, a pane cannot also be a loose sidebar row, because rows are
    /// generated from workspaces and each agent lives in exactly one.
    private func expectInvariant(_ result: [Workspace], _ agents: [LiveAgent]) {
        let placed = result.flatMap(\.agentIds)
        #expect(Set(placed) == Set(agents.map(\.id)))
        #expect(placed.count == agents.count, "an agent appears in more than one pane")
        #expect(result.allSatisfy { !$0.agentIds.isEmpty }, "a workspace holds no agents")
        #expect(Set(result.map(\.id)).count == result.count, "duplicate workspace ids")
    }

    @Test func givenNoStoredLayoutEachAgentBecomesItsOwnWorkspace() {
        let agents = live(root: ["ag-a", "ag-b"], worktrees: [("wt-1", ["ag-c"])])
        let result = WorkspaceReconciler.reconcile(stored: [], live: agents)

        expectInvariant(result, agents)
        #expect(result.count == 3)
        #expect(result.allSatisfy { $0.paneCount == 1 })
    }

    @Test func givenWorktreeAgentShouldRecordItsWorktreeContainer() {
        let agents = live(root: ["ag-a"], worktrees: [("wt-1", ["ag-c"])])
        let result = WorkspaceReconciler.reconcile(stored: [], live: agents)

        #expect(result.first { $0.agentIds == ["ag-c"] }?.container == .worktree("wt-1"))
        #expect(result.first { $0.agentIds == ["ag-a"] }?.container == .projectRoot)
    }

    // MARK: - Restart

    /// The regression this model was built for: a multi-pane workspace used to dissolve
    /// into one sidebar row per pane on relaunch, because the saved layout was never read.
    @Test func givenSavedMultiPaneWorkspaceShouldStayOneRowAcrossRestart() {
        withTempProject { root in
            let original = makeGroupedWorkspace()
            WorkspacePersistence.save([original], projectRoot: root)

            let agents = live(root: ["ag-a", "ag-b", "ag-c"])
            let restored = WorkspaceReconciler.reconcile(
                stored: WorkspacePersistence.load(projectRoot: root), live: agents)

            expectInvariant(restored, agents)
            #expect(restored.count == 1)
            #expect(restored.first?.paneCount == 3)
        }
    }

    @Test func givenSavedLayoutShouldPreservePaneIdsAcrossRestart() {
        withTempProject { root in
            let original = makeGroupedWorkspace()
            WorkspacePersistence.save([original], projectRoot: root)

            let restored = WorkspacePersistence.load(projectRoot: root)

            #expect(Set(restored.first?.root.allLeafIds ?? []) == Set(original.root.allLeafIds))
            #expect(restored.first?.focusedLeafId == original.focusedLeafId)
        }
    }

    @Test func givenSameInputsReconcileShouldBeDeterministicAndIdempotent() {
        let agents = live(root: ["ag-a", "ag-b", "ag-c"])
        let once = WorkspaceReconciler.reconcile(stored: [makeGroupedWorkspace()], live: agents)
        let again = WorkspaceReconciler.reconcile(stored: [makeGroupedWorkspace()], live: agents)

        #expect(once == again)
        #expect(WorkspaceReconciler.reconcile(stored: once, live: agents) == once)
    }

    // MARK: - Agents coming and going

    @Test func givenDeadAgentShouldCollapseItsPaneAndKeepTheWorkspace() {
        let agents = live(root: ["ag-a", "ag-c"])
        let result = WorkspaceReconciler.reconcile(stored: [makeGroupedWorkspace()], live: agents)

        expectInvariant(result, agents)
        #expect(result.count == 1)
        #expect(result.first?.paneCount == 2)
    }

    @Test func givenAllAgentsDeadShouldDropTheWorkspaceEntirely() {
        #expect(WorkspaceReconciler.reconcile(stored: [makeGroupedWorkspace()], live: []).isEmpty)
    }

    @Test func givenUnknownAgentShouldAdoptItWithoutDisturbingExistingGroups() {
        let agents = live(root: ["ag-a", "ag-b", "ag-c", "ag-new"])
        let result = WorkspaceReconciler.reconcile(stored: [makeGroupedWorkspace()], live: agents)

        expectInvariant(result, agents)
        #expect(result.count == 2)
        #expect(result[0].paneCount == 3)
        #expect(result[1].agentIds == ["ag-new"])
    }

    @Test func givenAgentClaimedByTwoWorkspacesShouldKeepOnlyTheFirst() {
        let duplicate = Workspace(
            id: "ws-duplicate", container: .projectRoot, root: .leaf(id: 0),
            panes: [0: Pane(tabs: [Surface(id: 0, content: .agent("ag-a"))], activeTabId: 0)],
            focusedLeafId: 0, nextLeafId: 1)
        let agents = live(root: ["ag-a"])

        let result = WorkspaceReconciler.reconcile(
            stored: [.adopting(agentId: "ag-a", container: .projectRoot), duplicate], live: agents)

        expectInvariant(result, agents)
        #expect(result.count == 1)
    }

    @Test func givenDeliberatelyEmptyPaneShouldSurviveAlongsideAnAgent() {
        var workspace = Workspace.adopting(agentId: "ag-a", container: .projectRoot)
        workspace.split(leafId: 0, axis: .vertical)  // empty pane awaiting a spawn
        let agents = live(root: ["ag-a"])

        let result = WorkspaceReconciler.reconcile(stored: [workspace], live: agents)

        expectInvariant(result, agents)
        #expect(result.first?.paneCount == 2)
    }

    @Test func givenFocusOnARemovedPaneShouldRefocusASurvivingOne() {
        var workspace = makeGroupedWorkspace()
        workspace.focusedLeafId = workspace.location(ofAgent: "ag-b")!.leafId

        let result = WorkspaceReconciler.reconcile(stored: [workspace], live: live(root: ["ag-a", "ag-c"]))

        let survivor = try? #require(result.first)
        #expect(survivor?.root.allLeafIds.contains(survivor!.focusedLeafId) == true)
    }

    // MARK: - Tabs

    @Test func givenDeadAgentInABackgroundTabShouldDropTheTabAndKeepThePane() {
        var workspace = Workspace.adopting(agentId: "ag-a", container: .projectRoot)
        workspace.newTab(leafId: 0, content: .agent("ag-b"))
        workspace.selectTab(0)
        let agents = live(root: ["ag-a"])

        let result = WorkspaceReconciler.reconcile(stored: [workspace], live: agents)

        expectInvariant(result, agents)
        #expect(result.first?.paneCount == 1)
        #expect(result.first?.panes[0]?.tabs.map(\.content) == [.agent("ag-a")])
    }

    @Test func givenDeadActiveTabShouldActivateItsLeftNeighbour() {
        var workspace = Workspace.adopting(agentId: "ag-a", container: .projectRoot)
        workspace.newTab(leafId: 0, content: .agent("ag-b"))
        let agents = live(root: ["ag-a"])

        let result = WorkspaceReconciler.reconcile(stored: [workspace], live: agents)

        #expect(result.first?.focusedAgentId == "ag-a")
    }

    @Test func givenAgentInTwoTabsShouldKeepOnlyTheFirst() {
        var workspace = Workspace.adopting(agentId: "ag-a", container: .projectRoot)
        workspace.newTab(leafId: 0, content: .agent("ag-a"))
        let agents = live(root: ["ag-a"])

        let result = WorkspaceReconciler.reconcile(stored: [workspace], live: agents)

        expectInvariant(result, agents)
        #expect(result.first?.tabCount == 1)
    }

    @Test func givenSavedTabsShouldRestoreStacksAndActiveTabsAcrossRestart() {
        withTempProject { root in
            var original = makeGroupedWorkspace()
            original.newTab(leafId: 0, content: .file(path: "/tmp/notes.md"))
            original.newTab(leafId: 0, content: .agent("ag-d"))
            WorkspacePersistence.save([original], projectRoot: root)

            let agents = live(root: ["ag-a", "ag-b", "ag-c", "ag-d"])
            let restored = WorkspaceReconciler.reconcile(
                stored: WorkspacePersistence.load(projectRoot: root), live: agents)

            expectInvariant(restored, agents)
            #expect(restored == [original])
        }
    }

    @Test func givenOnlyFileTabsLeftShouldKeepTheWorkspace() {
        var workspace = Workspace.adopting(agentId: "ag-a", container: .worktree("wt-1"))
        workspace.newTab(leafId: 0, content: .file(path: "/tmp/notes.md"))

        let result = WorkspaceReconciler.reconcile(stored: [workspace], live: [])

        #expect(result.count == 1)
        #expect(result.first?.panes[0]?.tabs.map(\.content) == [.file(path: "/tmp/notes.md")])
        #expect(result.first?.container == .worktree("wt-1"))
    }

    @Test func givenOnlyEmptyTabsLeftShouldDropTheWorkspaceUnlessASpawnIsPending() {
        var workspace = Workspace.adopting(agentId: "ag-a", container: .projectRoot)
        workspace.newTab(leafId: 0)

        #expect(WorkspaceReconciler.reconcile(stored: [workspace], live: []).isEmpty)
        #expect(WorkspaceReconciler.reconcile(stored: [workspace], live: [], reserved: [workspace.id]).count == 1)
    }

    // MARK: - Migration

    /// Version 2 stored one agent per leaf and file panes in a side list. Each leaf becomes a
    /// one-tab pane: its agent, else its file, else an empty tab.
    @Test func givenVersionTwoDocumentShouldMigrateEachLeafToOneTab() {
        withTempProject { root in
            let v2 = """
                {"version":2,"workspaces":[{"id":"ws-ag-a","focusedLeafId":2,"nextLeafId":3,
                "filePanes":[{"leafId":1,"path":"/tmp/x.md"}],
                "tree":{"type":"split","axis":"vertical","ratio":0.5,
                  "first":{"type":"leaf","leafId":0,"agentId":"ag-a"},
                  "second":{"type":"split","axis":"horizontal","ratio":0.5,
                    "first":{"type":"leaf","leafId":1},
                    "second":{"type":"leaf","leafId":2}}}}]}
                """
            try? v2.write(toFile: WorkspacePersistence.filePath(projectRoot: root), atomically: true, encoding: .utf8)

            let workspace = WorkspacePersistence.load(projectRoot: root).first

            #expect(workspace?.root.allLeafIds == [0, 1, 2])
            #expect(workspace?.panes[0]?.tabs.map(\.content) == [.agent("ag-a")])
            #expect(workspace?.panes[1]?.tabs.map(\.content) == [.file(path: "/tmp/x.md")])
            #expect(workspace?.panes[2]?.tabs.map(\.content) == [.empty])
            #expect(workspace?.focusedLeafId == 2)
            #expect(Set(workspace?.surfaces.map(\.surface.id) ?? []).count == 3)
        }
    }

    @Test func givenLegacyGridLayoutShouldMigrateIntoOneWorkspaceAndRetireTheFile() {
        withTempProject { root in
            let legacyPath = root + "/.pu/grid-layout.json"
            let legacy = """
                {"ownerAgentId":"ag-a","tree":{"type":"split","axis":"vertical","ratio":0.5,\
                "first":{"type":"leaf","agentId":"ag-a"},"second":{"type":"leaf","agentId":"ag-b"}}}
                """
            try? legacy.write(toFile: legacyPath, atomically: true, encoding: .utf8)

            let agents = live(root: ["ag-a", "ag-b"])
            let result = WorkspaceReconciler.reconcile(
                stored: WorkspacePersistence.load(projectRoot: root), live: agents)

            expectInvariant(result, agents)
            #expect(result.count == 1)
            #expect(result.first?.paneCount == 2)
            #expect(!FileManager.default.fileExists(atPath: legacyPath))
        }
    }

    @Test func givenNewerDocumentOnDiskShouldNotOverwriteIt() {
        withTempProject { root in
            let path = WorkspacePersistence.filePath(projectRoot: root)
            let newer = #"{"version":99,"workspaces":[],"somethingNew":true}"#
            try? newer.write(toFile: path, atomically: true, encoding: .utf8)

            WorkspacePersistence.save([makeGroupedWorkspace()], projectRoot: root)

            #expect((try? String(contentsOfFile: path, encoding: .utf8)) == newer)
        }
    }

    @Test func givenActiveWorkspaceShouldRecordItForTheCLI() throws {
        try withTempProject { root in
            WorkspacePersistence.save([makeGroupedWorkspace()], projectRoot: root, activeWorkspaceId: "ws-ag-a")
            let data = try Data(contentsOf: URL(fileURLWithPath: WorkspacePersistence.filePath(projectRoot: root)))
            let document = try JSONDecoder().decode(PersistedWorkspaceDocument.self, from: data)
            #expect(document.activeWorkspaceId == "ws-ag-a")
        }
    }

    @Test func givenMissingLayoutFileShouldLoadEmptyRatherThanFail() {
        withTempProject { root in
            #expect(WorkspacePersistence.load(projectRoot: root).isEmpty)
        }
    }
}
