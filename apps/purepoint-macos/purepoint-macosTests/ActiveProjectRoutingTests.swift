import Foundation
import Testing

@testable import PurePoint

// Minimal mock — avoids daemon connections in tests.
private struct StubWorkspaceService: WorkspaceService {
    func loadWorkspace(projectRoot: String) async throws -> WorkspaceSnapshot {
        WorkspaceSnapshot(worktrees: [], rootAgents: [])
    }
    func manifestPath(projectRoot: String) -> String { "/dev/null" }
}

@MainActor private func makeAgent(id: String) -> AgentModel {
    AgentModel(id: id, name: id, agentType: "claude", status: .running, prompt: "", startedAt: "")
}

@MainActor private func makeProject(
    root: String,
    agentIds: [String] = [],
    worktreeId: String? = nil,
    worktreeAgentIds: [String] = []
) -> ProjectState {
    let project = ProjectState(projectRoot: root, service: StubWorkspaceService(), registry: nil)
    project.rootAgents = agentIds.map { makeAgent(id: $0) }
    if let wtId = worktreeId {
        let wt = WorktreeModel(
            id: wtId, name: "wt", path: "/tmp/wt", branch: "main", status: "active",
            agents: worktreeAgentIds.map { makeAgent(id: $0) }
        )
        project.worktrees = [wt]
    }
    return project
}

/// Build a registry holding one single-pane workspace per agent, mirroring what
/// reconcile produces for a project whose agents have never been grouped.
@MainActor private func makeRegistry(_ projects: [ProjectState]) -> WorkspaceRegistry {
    let registry = WorkspaceRegistry()
    for project in projects {
        registry.reconcile(
            projectRoot: project.projectRoot,
            rootAgents: project.rootAgents,
            worktrees: project.worktrees
        )
    }
    return registry
}

@MainActor private func workspaceId(_ registry: WorkspaceRegistry, forAgent agentId: String) -> String {
    registry.workspaceId(forAgent: agentId) ?? ""
}

// MARK: - updateActiveProject routing

@Suite(.serialized)
@MainActor
struct ActiveProjectRoutingTests {

    @Test func workspaceSelectionSetsActiveProjectRoot() {
        let state = AppState(service: StubWorkspaceService())
        state.projects = [
            makeProject(root: "/a", agentIds: ["a1"]),
            makeProject(root: "/b", agentIds: ["b1"]),
        ]
        state.registry = makeRegistry(state.projects)

        state.updateActiveProject(for: .workspace(workspaceId(state.registry!, forAgent: "b1")))

        #expect(state.activeProjectRoot == "/b")
    }

    @Test func worktreeSelectionSetsActiveProjectRoot() {
        let state = AppState(service: StubWorkspaceService())
        state.projects = [
            makeProject(root: "/a"),
            makeProject(root: "/b", worktreeId: "wt-1"),
        ]

        state.updateActiveProject(for: .worktree("wt-1"))

        #expect(state.activeProjectRoot == "/b")
    }

    @Test func projectSelectionSetsActiveProjectRoot() {
        let state = AppState(service: StubWorkspaceService())
        state.projects = [makeProject(root: "/a"), makeProject(root: "/b")]

        state.updateActiveProject(for: .project("/b"))

        #expect(state.activeProjectRoot == "/b")
    }

    @Test func navSelectionKeepsExistingActiveProject() {
        let state = AppState(service: StubWorkspaceService())
        state.activeProjectRoot = "/a"

        state.updateActiveProject(for: .nav(.dashboard))

        #expect(state.activeProjectRoot == "/a")
    }

    @Test func nilSelectionKeepsExistingActiveProject() {
        let state = AppState(service: StubWorkspaceService())
        state.activeProjectRoot = "/a"

        state.updateActiveProject(for: nil)

        #expect(state.activeProjectRoot == "/a")
    }

    @Test func updateAlsoSetsActiveSidebarSelection() {
        let state = AppState(service: StubWorkspaceService())
        state.projects = [makeProject(root: "/a", agentIds: ["a1"])]
        state.registry = makeRegistry(state.projects)
        let wsId = workspaceId(state.registry!, forAgent: "a1")

        state.updateActiveProject(for: .workspace(wsId))

        #expect(state.activeSidebarSelection == .workspace(wsId))
    }

    @Test func unknownWorktreeIdPreservesActiveProjectRoot() {
        let state = AppState(service: StubWorkspaceService())
        state.projects = [makeProject(root: "/a"), makeProject(root: "/b")]
        state.activeProjectRoot = "/a"

        state.updateActiveProject(for: .worktree("nonexistent"))

        #expect(state.activeProjectRoot == "/a")
    }

    @Test func unknownWorkspaceIdPreservesActiveProjectRoot() {
        let state = AppState(service: StubWorkspaceService())
        state.projects = [makeProject(root: "/a"), makeProject(root: "/b")]
        state.registry = makeRegistry(state.projects)
        state.activeProjectRoot = "/a"

        state.updateActiveProject(for: .workspace("ws-nonexistent"))

        #expect(state.activeProjectRoot == "/a")
    }

    @Test func worktreeAgentWorkspaceRoutesThroughItsProject() {
        let state = AppState(service: StubWorkspaceService())
        state.projects = [
            makeProject(root: "/a"),
            makeProject(root: "/b", worktreeId: "wt-1", worktreeAgentIds: ["wt-agent"]),
        ]
        state.registry = makeRegistry(state.projects)

        state.updateActiveProject(for: .workspace(workspaceId(state.registry!, forAgent: "wt-agent")))

        #expect(state.activeProjectRoot == "/b")
    }
}

// MARK: - openProject initializes activeProjectRoot

@Suite(.serialized)
@MainActor
struct OpenProjectActiveRootTests {

    @Test func firstProjectSetsActiveProjectRoot() {
        let state = AppState(service: StubWorkspaceService())
        #expect(state.activeProjectRoot == nil)

        // Manually add project (avoiding startWatching daemon calls)
        let project = makeProject(root: "/first")
        state.projects.append(project)
        state.initializeActiveProjectIfNeeded(root: "/first")

        #expect(state.activeProjectRoot == "/first")
    }

    @Test func secondProjectDoesNotOverrideActiveProjectRoot() {
        let state = AppState(service: StubWorkspaceService())
        state.activeProjectRoot = "/first"

        state.initializeActiveProjectIfNeeded(root: "/second")

        #expect(state.activeProjectRoot == "/first")
    }
}
