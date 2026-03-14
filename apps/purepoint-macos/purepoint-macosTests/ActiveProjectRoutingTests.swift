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
    let project = ProjectState(projectRoot: root, service: StubWorkspaceService(), gridState: nil)
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

// MARK: - updateActiveProject routing

@Suite(.serialized)
@MainActor
struct ActiveProjectRoutingTests {

    @Test func agentSelectionSetsActiveProjectRoot() {
        let state = AppState(service: StubWorkspaceService())
        state.projects = [
            makeProject(root: "/a", agentIds: ["a1"]),
            makeProject(root: "/b", agentIds: ["b1"]),
        ]

        state.updateActiveProject(for: .agent("b1"))

        #expect(state.activeProjectRoot == "/b")
    }

    @Test func terminalSelectionSetsActiveProjectRoot() {
        let state = AppState(service: StubWorkspaceService())
        state.projects = [
            makeProject(root: "/a", agentIds: ["a1"]),
            makeProject(root: "/b", agentIds: ["b1"]),
        ]

        state.updateActiveProject(for: .terminal("b1"))

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

        state.updateActiveProject(for: .agent("a1"))

        #expect(state.activeSidebarSelection == .agent("a1"))
    }

    @Test func unknownWorktreeIdPreservesActiveProjectRoot() {
        let state = AppState(service: StubWorkspaceService())
        state.projects = [makeProject(root: "/a"), makeProject(root: "/b")]
        state.activeProjectRoot = "/a"

        state.updateActiveProject(for: .worktree("nonexistent"))

        #expect(state.activeProjectRoot == "/a")
    }

    @Test func unknownAgentIdPreservesActiveProjectRoot() {
        let state = AppState(service: StubWorkspaceService())
        state.projects = [makeProject(root: "/a"), makeProject(root: "/b")]
        state.activeProjectRoot = "/a"

        state.updateActiveProject(for: .agent("nonexistent"))

        #expect(state.activeProjectRoot == "/a")
    }

    @Test func worktreeAgentTerminalRoutesThroughWorktree() {
        let state = AppState(service: StubWorkspaceService())
        state.projects = [
            makeProject(root: "/a"),
            makeProject(root: "/b", worktreeId: "wt-1", worktreeAgentIds: ["wt-agent"]),
        ]

        state.updateActiveProject(for: .terminal("wt-agent"))

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
