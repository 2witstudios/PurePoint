import Foundation
import Testing

@testable import PurePoint

@MainActor @Suite struct PointGuardWorkspaceTests {
    private func withRegistry(_ body: (WorkspaceRegistry, String) throws -> Void) rethrows {
        let root = NSTemporaryDirectory() + "pp-point-guard-workspace-" + UUID().uuidString
        let registry = WorkspaceRegistry()
        defer {
            registry.forgetProject(root)
            try? FileManager.default.removeItem(atPath: root)
        }
        try body(registry, root)
    }

    @Test func chatCanSplitAndShareTabsWithRootAgents() throws {
        try withRegistry { registry, root in
            let id = registry.ensurePointGuardWorkspace(projectRoot: root)
            registry.split(workspaceId: id, leafId: 0, axis: .vertical)
            let target = try #require(registry.workspace(id: id)?.focusedSurface?.id)
            registry.reserveSurface(projectRoot: root, workspaceId: id, surfaceId: target)
            registry.fulfillReservation(projectRoot: root, workspaceId: id, surfaceId: target, agentId: "ag-terminal")
            registry.reconcile(
                projectRoot: root,
                rootAgents: [
                    AgentModel(
                        id: "ag-terminal", name: "Terminal", agentType: "terminal", status: .running, prompt: "",
                        startedAt: "")
                ], worktrees: [])
            let workspace = try #require(registry.workspace(id: id))
            #expect(workspace.container == .projectRoot)
            #expect(workspace.paneCount == 2)
            #expect(workspace.surfaces.map(\.surface.content) == [.pointGuard, .agent("ag-terminal")])
            #expect(registry.workspaceId(forAgent: "ag-terminal") == id)
            #expect(registry.workspacesByProject[root]?.count == 1)
        }
    }

    @Test func visitingPointGuardKeepsTheFocusedChatAndExistingLayout() throws {
        try withRegistry { registry, root in
            let id = registry.ensurePointGuardWorkspace(projectRoot: root)
            let terminal = try #require(registry.newTab(workspaceId: id, leafId: 0, content: .agent("ag-terminal")))
            registry.selectTab(workspaceId: id, surfaceId: 0)
            registry.activate(workspaceId: id)
            #expect(registry.activeWorkspace?.focusedSurface?.content == .pointGuard)
            #expect(registry.ensurePointGuardWorkspace(projectRoot: root) == id)
            #expect(registry.workspace(id: id)?.tabCount == 2)
            registry.selectTab(workspaceId: id, surfaceId: terminal)
            #expect(registry.activeWorkspace?.focusedSurface?.content == .agent("ag-terminal"))
        }
    }

    @Test func closingTheLastTabReturnsToChatWithoutReusingSurfaceIds() throws {
        try withRegistry { registry, root in
            let id = registry.ensurePointGuardWorkspace(projectRoot: root)
            registry.activeWorkspaceId = id
            registry.closeTab(workspaceId: id, surfaceId: 0)
            let workspace = try #require(registry.workspace(id: id))
            #expect(workspace.focusedSurface?.content == .pointGuard)
            #expect(workspace.focusedSurface?.id != 0)
            #expect(registry.activeWorkspaceId == id)
        }
    }

    @Test func deadTerminalDoesNotRemoveChatOrAdoptItAsAnAgent() {
        withRegistry { registry, root in
            let id = registry.ensurePointGuardWorkspace(projectRoot: root)
            registry.newTab(workspaceId: id, leafId: 0, content: .agent("ag-dead"))
            registry.reconcile(projectRoot: root, rootAgents: [], worktrees: [])
            #expect(registry.workspace(id: id)?.surfaces.map(\.surface.content) == [.pointGuard])
        }
    }

    @Test func closingChatDoesNotKillItsSiblingTerminalAndReopeningRestoresChat() {
        withRegistry { registry, root in
            let id = registry.ensurePointGuardWorkspace(projectRoot: root)
            registry.newTab(workspaceId: id, leafId: 0, content: .agent("ag-terminal"))
            var killed: [String] = []
            registry.onCloseAgent = { _, agentId in killed.append(agentId) }
            registry.closeTab(workspaceId: id, surfaceId: 0)
            #expect(killed.isEmpty)
            #expect(registry.workspace(id: id)?.agentIds == ["ag-terminal"])
            registry.ensurePointGuardWorkspace(projectRoot: root)
            #expect(registry.workspace(id: id)?.tabCount == 2)
            #expect(registry.workspace(id: id)?.focusedSurface?.content == .pointGuard)
        }
    }

    @Test func chatAndFileTabsPersistAcrossRestart() throws {
        try withRegistry { registry, root in
            let id = registry.ensurePointGuardWorkspace(projectRoot: root)
            registry.split(workspaceId: id, leafId: 0, axis: .horizontal)
            let file = try #require(registry.workspace(id: id)?.focusedSurface?.id)
            registry.openFile(workspaceId: id, surfaceId: file, path: root + "/notes.md")
            registry.activeWorkspaceId = id
            registry.saveAll()
            let restored = WorkspacePersistence.load(projectRoot: root)
            #expect(restored == [registry.workspace(id: id)!])
            #expect(restored.first?.isPointGuard == true)
            #expect(WorkspaceReconciler.reconcile(stored: restored, live: []) == restored)
        }
    }

    @Test func projectRootsHaveDistinctPointGuardLayouts() {
        withRegistry { registry, root in
            let first = registry.ensurePointGuardWorkspace(projectRoot: root)
            let secondRoot = root + "-second"
            defer { registry.forgetProject(secondRoot); try? FileManager.default.removeItem(atPath: secondRoot) }
            let second = registry.ensurePointGuardWorkspace(projectRoot: secondRoot)
            #expect(first != second)
            #expect(registry.projectRoot(forWorkspace: first) == root)
            #expect(registry.projectRoot(forWorkspace: second) == secondRoot)
        }
    }

    @Test func worktreeAgentsCannotBeClaimedByPointGuard() {
        var workspace = Workspace.pointGuard(projectRoot: "/tmp/project")
        workspace.newTab(leafId: 0, content: .agent("ag-worktree"))
        let restored = WorkspaceReconciler.reconcile(
            stored: [workspace], live: [LiveAgent(id: "ag-worktree", container: .worktree("wt-one"))])
        #expect(restored.first?.container == .projectRoot)
        #expect(restored.first?.surfaces.map(\.surface.content) == [.pointGuard])
        #expect(restored.last?.container == .worktree("wt-one"))
    }
}
