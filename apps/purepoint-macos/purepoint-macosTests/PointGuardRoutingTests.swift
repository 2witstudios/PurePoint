import Foundation
import Testing
@testable import PurePoint

private struct PointGuardRoutingService: WorkspaceService {
    func loadWorkspace(projectRoot: String) async throws -> WorkspaceSnapshot {
        WorkspaceSnapshot(worktrees: [], rootAgents: [])
    }
    func manifestPath(projectRoot: String) -> String { "/dev/null" }
}

@MainActor @Suite(.serialized) struct PointGuardRoutingTests {
    private func withState(_ body: (AppState, WorkspaceRegistry, String) throws -> Void) rethrows {
        let root = NSTemporaryDirectory() + "point-guard-routing-" + UUID().uuidString
        let defaults = UserDefaults.standard
        let keys = ["PurePointPointGuardFolders", "PurePointOpenProjects"]
        let saved = keys.map { defaults.object(forKey: $0) }
        let registry = WorkspaceRegistry()
        let state = AppState(service: PointGuardRoutingService(), startProject: { _ in })
        state.registry = registry
        state.updatePointGuardRoot(root)
        defer {
            for (key, value) in zip(keys, saved) { defaults.set(value, forKey: key) }
            for path in [root, root + "-project", root + "-next"] { registry.forgetProject(path) }
            for path in [root, root + "-project", root + "-next"] {
                try? FileManager.default.removeItem(atPath: path)
            }
        }
        try body(state, registry, root)
    }

    @Test func restoringOpenProjectsRetainsSavedHiddenPointGuardFolders() throws {
        try withState { state, _, root in
            try FileManager.default.createDirectory(atPath: root, withIntermediateDirectories: true)
            try FileManager.default.createDirectory(atPath: root + "-project", withIntermediateDirectories: true)
            UserDefaults.standard.set([root + "-project"], forKey: "PurePointOpenProjects")
            UserDefaults.standard.set([root], forKey: "PurePointPointGuardFolders")
            state.restoreProjects()
            #expect(state.projects.map(\.projectRoot) == [root + "-project"])
            #expect(state.projectState(forRoot: root) != nil)
            #expect(UserDefaults.standard.stringArray(forKey: "PurePointPointGuardFolders") == [root])
        }
    }

    @Test func workspaceCreationWaitsForPiWorkingFolder() {
        let registry = WorkspaceRegistry()
        var startedRoots: [String] = []
        let state = AppState(service: PointGuardRoutingService(), startProject: {
            startedRoots.append($0.projectRoot)
        })
        state.registry = registry
        state.updatePointGuardRoot("")
        #expect(state.pointGuardRoot == nil)
        #expect(state.activatePointGuardWorkspace() == nil)
        state.newPointGuardTab()
        #expect(startedRoots.isEmpty)
        #expect(registry.workspacesByProject.isEmpty)
        #expect(registry.activeWorkspaceId == nil)
    }

    @Test func restoringOrSelectingPointGuardPreservesVisibleCodingProject() throws {
        try withState { state, registry, root in
            state.openProject(root + "-project")
            let id = try #require(state.activatePointGuardWorkspace())
            state.pendingSelectWorkspaceId = id
            #expect(state.resolvePendingWorkspaceSelection() == id)
            #expect(state.activeProjectRoot == root + "-project")
            state.updateActiveProject(for: .workspace(id))
            #expect(state.activeProjectRoot == root + "-project")
            state.updateActiveProject(for: .nav(.dashboard))
            #expect(state.activeProjectRoot == root + "-project")
            #expect(registry.projectRoot(forWorkspace: id) == root)
        }
    }

    @Test func splitsUsePiFolderDespiteSelectedProject() throws {
        try withState { state, registry, root in
            state.openProject(root + "-project")
            state.activeProjectRoot = root + "-project"
            let id = try #require(state.activatePointGuardWorkspace())
            for axis in [PaneSplitNode.Axis.vertical, .horizontal] {
                let leaf = try #require(registry.workspace(id: id)?.focusedLeafId)
                registry.split(workspaceId: id, leafId: leaf, axis: axis)
                #expect(registry.projectRoot(forWorkspace: id) == root)
                #expect(registry.workspace(id: id)?.container == .projectRoot)
                #expect(state.projectState(forRoot: root)?.projectRoot == root)
                #expect(state.fileRoot(forWorkspace: id, registry: registry) == root)
            }
            #expect(state.projects.map(\.projectRoot) == [root + "-project"])
            #expect(state.activeProjectRoot == root + "-project")
        }
    }

    @Test func pointGuardSupportsTabsWithoutAnOpenProjectAndFindsItsAgents() throws {
        try withState { state, registry, root in
            state.newPointGuardTab()
            let workspace = try #require(registry.activeWorkspace)
            #expect(registry.projectRoot(forWorkspace: workspace.id) == root)
            #expect(workspace.tabCount == 2)
            #expect(registry.pendingPaletteSurfaceId == workspace.focusedSurface?.id)
            #expect(state.projects.isEmpty)
            let project = try #require(state.projectState(forRoot: root))
            let agent = AgentModel(id: "ag-pg", name: "Terminal", agentType: "terminal",
                                   status: .running, prompt: "", startedAt: "")
            project.rootAgents = [agent]
            #expect(state.agent(byId: agent.id)?.id == agent.id)
            #expect(state.projectState(forAgentId: agent.id) === project)
        }
    }

    @Test func changingPiFolderCreatesTheNextTabThereAndKeepsPreviousState() throws {
        try withState { state, registry, root in
            let original = try #require(state.activatePointGuardWorkspace())
            state.updatePointGuardRoot(root + "-next")
            state.newPointGuardTab()
            let next = try #require(registry.activeWorkspace)
            #expect(next.id != original)
            #expect(registry.projectRoot(forWorkspace: next.id) == root + "-next")
            #expect(state.projectState(forRoot: root) != nil)
            #expect(registry.workspace(id: original) != nil)
            state.updatePointGuardRoot("")
            #expect(state.pointGuardRoot == root + "-next")
        }
    }

    @Test func removingPointGuardProjectPreservesOwnerAndSelectsAnotherVisibleProject() throws {
        try withState { state, _, root in
            _ = try #require(state.activatePointGuardWorkspace())
            let owner = try #require(state.projectState(forRoot: root))
            state.openProject(root)
            state.openProject(root + "-project")
            state.activeProjectRoot = root
            #expect(state.closeProject(root) == false)
            #expect(state.projectState(forRoot: root) === owner)
            #expect(state.activeProjectRoot == root + "-project")
            #expect(state.closeProject(root + "-project") == true)
            #expect(state.activeProjectRoot == nil)
        }
    }

    @Test func openingAndClosingPiFolderAsAProjectKeepsTheSameAgentOwner() throws {
        try withState { state, _, root in
            _ = try #require(state.activatePointGuardWorkspace())
            let owner = try #require(state.projectState(forRoot: root))
            state.openProject(root)
            #expect(state.projects.first === owner)
            #expect(state.closeProject(root) == false)
            #expect(state.activeProjectRoot == nil)
            #expect(state.projects.isEmpty)
            #expect(state.projectState(forRoot: root) === owner)
        }
    }
}
