import Foundation
import Testing

@testable import PurePoint

private struct RegistryStubService: WorkspaceService {
    func loadWorkspace(projectRoot: String) async throws -> WorkspaceSnapshot {
        WorkspaceSnapshot(worktrees: [], rootAgents: [])
    }
    func manifestPath(projectRoot: String) -> String { "/dev/null" }
}

private func agent(_ id: String) -> AgentModel {
    AgentModel(id: id, name: id, agentType: "claude", status: .running, prompt: "", startedAt: "")
}

@MainActor private func makeRegistry(root: String, agents: [String]) -> WorkspaceRegistry {
    let registry = WorkspaceRegistry()
    registry.reconcile(projectRoot: root, rootAgents: agents.map(agent), worktrees: [])
    return registry
}

@MainActor @Suite
struct WorkspaceRegistryTests {
    private let root = NSTemporaryDirectory() + "pp-registry-tests-" + UUID().uuidString

    // MARK: - Reservations

    @Test func pendingReservationDoesNotAdoptUnrelatedAgent() {
        let registry = makeRegistry(root: root, agents: ["ag-a"])
        registry.reservePane(projectRoot: root, workspaceId: "ws-ag-a", leafId: 0)

        registry.reconcile(projectRoot: root, rootAgents: [agent("ag-a"), agent("ag-x")], worktrees: [])

        #expect(registry.workspaces(forProject: root).map(\.id) == ["ws-ag-a"])
        #expect(registry.workspaceId(forAgent: "ag-x") == nil)
    }

    @Test func fulfillingByAgentIdPlacesTheBoundAgentNotTheFirstLiveOne() {
        let registry = makeRegistry(root: root, agents: ["ag-a"])
        registry.split(workspaceId: "ws-ag-a", leafId: 0, axis: .vertical)
        let newLeaf = registry.workspace(id: "ws-ag-a")!.focusedLeafId
        registry.reservePane(projectRoot: root, workspaceId: "ws-ag-a", leafId: newLeaf)

        registry.reconcile(projectRoot: root, rootAgents: [agent("ag-a"), agent("ag-x"), agent("ag-new")], worktrees: [])
        registry.fulfillReservation(projectRoot: root, workspaceId: "ws-ag-a", leafId: newLeaf, agentId: "ag-new")

        #expect(registry.workspace(id: "ws-ag-a")?.root.agentId(forLeafId: newLeaf) == "ag-new")
        #expect(registry.workspaceId(forAgent: "ag-x") == "ws-ag-x")
        #expect(registry.workspaces(forProject: root).map(\.id) == ["ws-ag-a", "ws-ag-x"])
    }

    @Test func releasedReservationLetDeferredAgentsSurface() {
        let registry = makeRegistry(root: root, agents: ["ag-a"])
        registry.reservePane(projectRoot: root, workspaceId: "ws-ag-a", leafId: 0)
        registry.reconcile(projectRoot: root, rootAgents: [agent("ag-a"), agent("ag-x")], worktrees: [])

        registry.releaseReservation(projectRoot: root, workspaceId: "ws-ag-a", leafId: 0)

        #expect(registry.workspaceId(forAgent: "ag-x") == "ws-ag-x")
    }

    // MARK: - setAgent

    @Test func bindingAnAgentRemovesItsOtherPaneClaims() {
        let registry = makeRegistry(root: root, agents: ["ag-a", "ag-b"])
        registry.split(workspaceId: "ws-ag-b", leafId: 0, axis: .vertical)
        let newLeaf = registry.workspace(id: "ws-ag-b")!.focusedLeafId

        registry.setAgent("ag-a", workspaceId: "ws-ag-b", leafId: newLeaf)

        #expect(registry.workspaceId(forAgent: "ag-a") == "ws-ag-b")
        #expect(registry.workspace(id: "ws-ag-a") == nil)
        #expect(registry.workspace(id: "ws-ag-b")?.agentIds == ["ag-b", "ag-a"])
    }

    // MARK: - Remote commands

    @Test func remoteSplitAndCloseIgnoreUnknownLeaves() {
        let registry = makeRegistry(root: root, agents: ["ag-a"])
        registry.activeWorkspaceId = "ws-ag-a"
        let before = registry.workspace(id: "ws-ag-a")

        registry.handleRemoteCommand(.split(leafId: 99, axis: "v"), from: root)
        registry.handleRemoteCommand(.close(leafId: 99), from: root)

        #expect(registry.workspace(id: "ws-ag-a") == before)
    }

    @Test func remoteSplitWithoutLeafUsesFocusedLeaf() {
        let registry = makeRegistry(root: root, agents: ["ag-a"])
        registry.activeWorkspaceId = "ws-ag-a"

        registry.handleRemoteCommand(.split(leafId: nil, axis: "v"), from: root)

        #expect(registry.workspace(id: "ws-ag-a")?.paneCount == 2)
    }

    // MARK: - Pending workspace restore

    @Test func pendingWorkspaceSurvivesUntilItsProjectReconciles() {
        let registry = WorkspaceRegistry()
        let appState = AppState(service: RegistryStubService())
        appState.registry = registry
        // Appended directly: openProject would start a real daemon watcher.
        appState.projects.append(ProjectState(projectRoot: root, service: RegistryStubService(), registry: registry))
        appState.pendingSelectWorkspaceId = "ws-ag-a"

        #expect(appState.resolvePendingWorkspaceSelection() == nil)
        #expect(appState.pendingSelectWorkspaceId == "ws-ag-a")

        registry.reconcile(projectRoot: root, rootAgents: [agent("ag-a")], worktrees: [])

        #expect(appState.resolvePendingWorkspaceSelection() == "ws-ag-a")
        #expect(appState.pendingSelectWorkspaceId == nil)
        withExtendedLifetime(registry) {}  // AppState.registry is weak
    }

    @Test func pendingWorkspaceClearsOnceAllProjectsReconciledWithoutIt() {
        let registry = WorkspaceRegistry()
        let appState = AppState(service: RegistryStubService())
        appState.registry = registry
        // Appended directly: openProject would start a real daemon watcher.
        appState.projects.append(ProjectState(projectRoot: root, service: RegistryStubService(), registry: registry))
        appState.pendingSelectWorkspaceId = "ws-gone"

        registry.reconcile(projectRoot: root, rootAgents: [agent("ag-a")], worktrees: [])

        #expect(appState.resolvePendingWorkspaceSelection() == nil)
        #expect(appState.pendingSelectWorkspaceId == nil)
        withExtendedLifetime(registry) {}  // AppState.registry is weak
    }
}
