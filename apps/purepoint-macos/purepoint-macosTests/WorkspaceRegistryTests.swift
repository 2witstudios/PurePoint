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
        registry.reserveSurface(projectRoot: root, workspaceId: "ws-ag-a", surfaceId: 0)

        registry.reconcile(projectRoot: root, rootAgents: [agent("ag-a"), agent("ag-x")], worktrees: [])

        #expect(registry.workspaces(forProject: root).map(\.id) == ["ws-ag-a"])
        #expect(registry.workspaceId(forAgent: "ag-x") == nil)
    }

    @Test func fulfillingByAgentIdPlacesTheBoundAgentNotTheFirstLiveOne() {
        let registry = makeRegistry(root: root, agents: ["ag-a"])
        registry.split(workspaceId: "ws-ag-a", leafId: 0, axis: .vertical)
        let newTab = registry.workspace(id: "ws-ag-a")!.focusedSurface!.id
        registry.reserveSurface(projectRoot: root, workspaceId: "ws-ag-a", surfaceId: newTab)

        registry.reconcile(projectRoot: root, rootAgents: [agent("ag-a"), agent("ag-x"), agent("ag-new")], worktrees: [])
        registry.fulfillReservation(projectRoot: root, workspaceId: "ws-ag-a", surfaceId: newTab, agentId: "ag-new")

        #expect(registry.workspace(id: "ws-ag-a")?.surface(id: newTab)?.content == .agent("ag-new"))
        #expect(registry.workspaceId(forAgent: "ag-x") == "ws-ag-x")
        #expect(registry.workspaces(forProject: root).map(\.id) == ["ws-ag-a", "ws-ag-x"])
    }

    @Test func releasedReservationLetDeferredAgentsSurface() {
        let registry = makeRegistry(root: root, agents: ["ag-a"])
        registry.reserveSurface(projectRoot: root, workspaceId: "ws-ag-a", surfaceId: 0)
        registry.reconcile(projectRoot: root, rootAgents: [agent("ag-a"), agent("ag-x")], worktrees: [])

        registry.releaseReservation(projectRoot: root, workspaceId: "ws-ag-a", surfaceId: 0)

        #expect(registry.workspaceId(forAgent: "ag-x") == "ws-ag-x")
    }

    // MARK: - setAgent

    @Test func bindingAnAgentRemovesItsOtherTabClaims() {
        let registry = makeRegistry(root: root, agents: ["ag-a", "ag-b"])
        registry.split(workspaceId: "ws-ag-b", leafId: 0, axis: .vertical)
        let newTab = registry.workspace(id: "ws-ag-b")!.focusedSurface!.id

        registry.setAgent("ag-a", workspaceId: "ws-ag-b", surfaceId: newTab)

        #expect(registry.workspaceId(forAgent: "ag-a") == "ws-ag-b")
        #expect(registry.workspace(id: "ws-ag-a") == nil)
        #expect(registry.workspace(id: "ws-ag-b")?.agentIds == ["ag-b", "ag-a"])
    }

    @Test func bindingAnAgentAlreadyInAnotherTabOfTheSameWorkspaceMovesIt() {
        let registry = makeRegistry(root: root, agents: ["ag-a"])
        let newTab = registry.newTab(workspaceId: "ws-ag-a", leafId: 0)!

        registry.setAgent("ag-a", workspaceId: "ws-ag-a", surfaceId: newTab)

        let workspace = registry.workspace(id: "ws-ag-a")
        #expect(workspace?.agentIds == ["ag-a"])
        #expect(workspace?.surface(id: newTab)?.content == .agent("ag-a"))
        #expect(workspace?.tabCount == 1)
    }

    // MARK: - Tabs

    @Test func closingATabKillsOnlyItsAgent() {
        let registry = makeRegistry(root: root, agents: ["ag-a", "ag-b"])
        registry.setAgent("ag-b", workspaceId: "ws-ag-a", surfaceId: registry.newTab(workspaceId: "ws-ag-a", leafId: 0)!)
        var killed: [String] = []
        registry.onCloseAgent = { _, agentId in killed.append(agentId) }

        let tab = registry.workspace(id: "ws-ag-a")!.location(ofAgent: "ag-b")!.surfaceId
        registry.closeTab(workspaceId: "ws-ag-a", surfaceId: tab)

        #expect(killed == ["ag-b"])
        #expect(registry.workspace(id: "ws-ag-a")?.agentIds == ["ag-a"])
    }

    @Test func closingTheLastTabDropsTheWorkspace() {
        let registry = makeRegistry(root: root, agents: ["ag-a"])
        registry.activeWorkspaceId = "ws-ag-a"
        var killed: [String] = []
        registry.onCloseAgent = { _, agentId in killed.append(agentId) }

        registry.closeTab(workspaceId: "ws-ag-a", surfaceId: 0)

        #expect(killed == ["ag-a"])
        #expect(registry.workspace(id: "ws-ag-a") == nil)
        #expect(registry.activeWorkspaceId == nil)
    }

    @Test func closingAPaneKillsEveryAgentInItsTabs() {
        let registry = makeRegistry(root: root, agents: ["ag-a", "ag-b", "ag-c"])
        registry.split(workspaceId: "ws-ag-a", leafId: 0, axis: .vertical)
        let ws = registry.workspace(id: "ws-ag-a")!
        let newLeaf = ws.focusedLeafId
        registry.setAgent("ag-b", workspaceId: "ws-ag-a", surfaceId: ws.focusedSurface!.id)
        registry.setAgent("ag-c", workspaceId: "ws-ag-a", surfaceId: registry.newTab(workspaceId: "ws-ag-a", leafId: newLeaf)!)
        var killed: [String] = []
        registry.onCloseAgent = { _, agentId in killed.append(agentId) }

        registry.closePane(workspaceId: "ws-ag-a", leafId: newLeaf)

        #expect(Set(killed) == ["ag-b", "ag-c"])
        #expect(registry.workspace(id: "ws-ag-a")?.paneCount == 1)
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

    @Test func remoteNewTabWithAgentOpensItBesideTheActiveTab() {
        let registry = makeRegistry(root: root, agents: ["ag-a", "ag-b"])
        registry.activeWorkspaceId = "ws-ag-a"

        registry.handleRemoteCommand(.newTab(leafId: nil, agentId: "ag-b"), from: root)

        let workspace = registry.workspace(id: "ws-ag-a")
        #expect(workspace?.panes[0]?.tabs.map(\.content) == [.agent("ag-a"), .agent("ag-b")])
        #expect(workspace?.focusedAgentId == "ag-b")
        #expect(registry.workspace(id: "ws-ag-b") == nil)
    }

    @Test func remoteSelectTabUsesOneBasedPositionsAndCycles() {
        let registry = makeRegistry(root: root, agents: ["ag-a"])
        registry.activeWorkspaceId = "ws-ag-a"
        registry.newTab(workspaceId: "ws-ag-a", leafId: 0)

        registry.handleRemoteCommand(.selectTab(leafId: nil, index: 1, direction: nil), from: root)
        #expect(registry.workspace(id: "ws-ag-a")?.focusedAgentId == "ag-a")

        registry.handleRemoteCommand(.selectTab(leafId: nil, index: nil, direction: "prev"), from: root)
        #expect(registry.workspace(id: "ws-ag-a")?.focusedSurface?.content == .empty)
    }

    @Test func remoteSetAgentWithoutLeafTargetsTheFocusedPanesActiveTab() {
        let registry = makeRegistry(root: root, agents: ["ag-a", "ag-b"])
        registry.activeWorkspaceId = "ws-ag-a"
        registry.newTab(workspaceId: "ws-ag-a", leafId: 0)

        registry.handleRemoteCommand(.setAgent(leafId: nil, agentId: "ag-b"), from: root)

        #expect(registry.workspace(id: "ws-ag-a")?.agentIds == ["ag-a", "ag-b"])
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
