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
        let keys = ["PurePointPointGuardFolders", "PurePointOpenProjects", "PurePointShownPointGuardFolders"]
        let saved = keys.map { defaults.object(forKey: $0) }
        let registry = WorkspaceRegistry()
        let state = AppState(service: PointGuardRoutingService(), startProject: { _ in }, followsLocalDaemon: false)
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

    @Test func startupUsesPointGuardNewTabRoutingBeforePiReportsItsFolder() throws {
        try withState { state, registry, root in
            state.updatePointGuardRoot("")
            state.openProject(root + "-project")
            #expect(state.activeSidebarSelection == .nav(.dashboard))
            #expect(state.activeProjectRoot == root + "-project")
            state.newPointGuardTab()
            #expect(registry.activeWorkspaceId == nil)
            #expect(registry.workspacesByProject.isEmpty)
            #expect(state.projects.map(\.projectRoot) == [root + "-project"])
        }
    }

    @Test func paletteQueriesPointGuardFolderWithAndWithoutVisibleProjects() throws {
        try withState { state, _, root in
            let id = try #require(state.activatePointGuardWorkspace())
            #expect(state.paletteProjectRoots(forWorkspace: id) == [root])
            state.openProject(root + "-project")
            #expect(state.paletteProjectRoots(forWorkspace: id) == [root, root + "-project"])
            state.openProject(root)
            #expect(state.paletteProjectRoots(forWorkspace: id) == [root, root + "-project"])
        }
    }

    @Test func argumentLaunchRestoresHiddenFoldersWithoutRestoringOtherVisibleProjects() throws {
        try withState { state, _, root in
            try FileManager.default.createDirectory(atPath: root, withIntermediateDirectories: true)
            UserDefaults.standard.set([root + "-next"], forKey: "PurePointOpenProjects")
            UserDefaults.standard.set([root], forKey: "PurePointPointGuardFolders")
            state.restoreProjects(projectRoot: root + "-project")
            #expect(state.projects.map(\.projectRoot) == [root + "-project"])
            #expect(state.projectState(forRoot: root) != nil)
            #expect(state.projectState(forRoot: root + "-next") == nil)
        }
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

    @Test func unknownPointGuardFolderClearsPreviouslySelectedCodingWorkspace() throws {
        try withState { state, registry, root in
            let agent = AgentModel(id: "ag-coding", name: "Terminal", agentType: "terminal",
                                   status: .running, prompt: "", startedAt: "")
            registry.reconcile(projectRoot: root + "-project", rootAgents: [agent], worktrees: [])
            let id = try #require(registry.workspaceId(forAgent: agent.id))
            registry.activate(workspaceId: id)
            state.updatePointGuardRoot("")
            #expect(state.activatePointGuardWorkspace() == nil)
            #expect(registry.activeWorkspaceId == nil)
            #expect(registry.workspace(id: id) != nil)
        }
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
            #expect(state.pointGuardRoot == nil)
        }
    }

    @Test func removingConfiguredFolderBeforeWorkspaceLoadsRetainsItsState() throws {
        try withState { state, registry, root in
            state.openProject(root)
            let owner = try #require(state.projectState(forRoot: root))
            #expect(registry.workspacesByProject[root] == nil)
            #expect(state.closeProject(root) == false)
            #expect(state.projectState(forRoot: root) === owner)
            #expect(UserDefaults.standard.stringArray(forKey: "PurePointPointGuardFolders") == [root])
        }
    }

    @Test func removingHistoricalVisibleFolderBeforeLayoutRestoresRetainsItsState() throws {
        try withState { state, registry, root in
            try FileManager.default.createDirectory(atPath: root, withIntermediateDirectories: true)
            UserDefaults.standard.set([root], forKey: "PurePointOpenProjects")
            UserDefaults.standard.set([root], forKey: "PurePointPointGuardFolders")
            state.restoreProjects()
            state.updatePointGuardRoot(root + "-next")
            let owner = try #require(state.projectState(forRoot: root))
            #expect(registry.workspacesByProject[root] == nil)
            #expect(state.closeProject(root) == false)
            #expect(state.projectState(forRoot: root) === owner)
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

    // MARK: - The local daemon's projects decide the sidebar

    @Test func registeredProjectsOpenAndOnesRemovedElsewhereClose() throws {
        try withState { state, _, root in
            for path in [root + "-project", root + "-next"] {
                try FileManager.default.createDirectory(atPath: path, withIntermediateDirectories: true)
            }
            // The daemon reports canonical roots (`/private/var/...` for a temporary folder).
            let project = AppState.canonicalPath(root + "-project"), next = AppState.canonicalPath(root + "-next")
            state.applyLocalProjects([next, project])
            #expect(Set(state.projects.map(\.projectRoot)) == [project, next])
            // Removed on another device, or with `pu projects remove`.
            state.applyLocalProjects([project])
            #expect(state.projects.map(\.projectRoot) == [project])
        }
    }

    @Test func projectOpenedBeforeTheDaemonListsItStaysOpen() throws {
        try withState { state, _, root in
            try FileManager.default.createDirectory(atPath: root + "-project", withIntermediateDirectories: true)
            // Its `init` has not reached the daemon yet: absent from a list it was never in.
            state.openProject(root + "-project")
            state.applyLocalProjects([])
            #expect(state.projects.map(\.projectRoot) == [root + "-project"])
            // Opened by another path, it is the same project once the daemon lists it.
            state.applyLocalProjects([AppState.canonicalPath(root + "-project")])
            #expect(state.projects.map(\.projectRoot) == [root + "-project"])
        }
    }

    @Test func pointGuardFolderStaysOutOfTheSidebarUnlessOpened() throws {
        try withState { state, _, root in
            try FileManager.default.createDirectory(atPath: root, withIntermediateDirectories: true)
            UserDefaults.standard.removeObject(forKey: "PurePointOpenProjects")
            UserDefaults.standard.set([root], forKey: "PurePointPointGuardFolders")
            UserDefaults.standard.removeObject(forKey: "PurePointShownPointGuardFolders")
            state.restoreProjects()
            state.applyLocalProjects([AppState.canonicalPath(root)])
            #expect(state.projects.isEmpty)
            #expect(state.projectState(forRoot: root) != nil)
            state.openProject(root)
            #expect(UserDefaults.standard.stringArray(forKey: "PurePointShownPointGuardFolders") == [root])
        }
    }

    @Test func legacySidebarListIsImportedOnceThenDeleted() throws {
        try withState { state, _, root in
            try FileManager.default.createDirectory(atPath: root + "-project", withIntermediateDirectories: true)
            UserDefaults.standard.set([root + "-project"], forKey: "PurePointOpenProjects")
            state.restoreProjects()
            #expect(state.projects.map(\.projectRoot) == [root + "-project"])
            #expect(UserDefaults.standard.stringArray(forKey: "PurePointOpenProjects") == nil)
        }
    }

    @Test func projectsJoiningAfterLaunchKeepTheirSuspendedAgentsPaused() throws {
        try withState { state, _, root in
            for path in [root + "-project", root + "-next"] {
                try FileManager.default.createDirectory(atPath: path, withIntermediateDirectories: true)
            }
            UserDefaults.standard.removeObject(forKey: "PurePointOpenProjects")
            state.restoreProjects()
            // Last session's sidebar: this app suspended its agents on quit and resumes them.
            let project = AppState.canonicalPath(root + "-project"), next = AppState.canonicalPath(root + "-next")
            state.applyLocalProjects([project])
            #expect(state.projectState(forRoot: project)?.keepsSuspendedAgentsOnNextOpen == false)
            // Added later, by another device or the CLI.
            state.applyLocalProjects([next, project])
            #expect(state.projectState(forRoot: next)?.keepsSuspendedAgentsOnNextOpen == true)
        }
    }

    @Test func pointGuardFolderRemovedElsewhereIsRegisteredAgain() throws {
        var started: [String] = []
        let root = NSTemporaryDirectory() + "point-guard-reregister-" + UUID().uuidString
        try FileManager.default.createDirectory(atPath: root, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(atPath: root) }
        let saved = UserDefaults.standard.object(forKey: "PurePointPointGuardFolders")
        let savedShown = UserDefaults.standard.object(forKey: "PurePointShownPointGuardFolders")
        defer {
            UserDefaults.standard.set(saved, forKey: "PurePointPointGuardFolders")
            UserDefaults.standard.set(savedShown, forKey: "PurePointShownPointGuardFolders")
        }
        let state = AppState(
            service: PointGuardRoutingService(), startProject: { started.append($0.projectRoot) },
            followsLocalDaemon: false)
        state.updatePointGuardRoot(root)
        state.openProject(root)
        let canonical = AppState.canonicalPath(root)
        state.applyLocalProjects([canonical])
        started.removeAll()
        state.applyLocalProjects([])
        #expect(started == [root])
        #expect(state.projects.map(\.projectRoot) == [root])
    }
}
