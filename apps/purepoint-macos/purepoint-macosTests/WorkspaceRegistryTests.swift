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

    @Test func closingTheLastAgentTabKeepsAWorkspaceThatStillHasAFileTab() {
        let registry = makeRegistry(root: root, agents: ["ag-a"])
        registry.newTab(workspaceId: "ws-ag-a", leafId: 0, content: .file(path: "/tmp/x.md"))

        registry.closeTab(workspaceId: "ws-ag-a", surfaceId: 0)
        registry.reconcile(projectRoot: root, rootAgents: [], worktrees: [])

        #expect(registry.workspace(id: "ws-ag-a")?.panes[0]?.tabs.map(\.content) == [.file(path: "/tmp/x.md")])
    }

    @Test func closingTheLastAgentTabBesideAnEmptyTabDropsTheWorkspaceAtOnce() {
        let registry = makeRegistry(root: root, agents: ["ag-a"])
        registry.newTab(workspaceId: "ws-ag-a", leafId: 0)

        registry.closeTab(workspaceId: "ws-ag-a", surfaceId: 0)

        #expect(registry.workspace(id: "ws-ag-a") == nil)
    }

    @Test func anEmptyTabWaitingForASpawnKeepsItsWorkspace() {
        let registry = makeRegistry(root: root, agents: ["ag-a"])
        let pending = registry.newTab(workspaceId: "ws-ag-a", leafId: 0)!
        registry.reserveSurface(projectRoot: root, workspaceId: "ws-ag-a", surfaceId: pending)

        registry.closeTab(workspaceId: "ws-ag-a", surfaceId: 0)
        registry.reconcile(projectRoot: root, rootAgents: [], worktrees: [])
        registry.fulfillReservation(projectRoot: root, workspaceId: "ws-ag-a", surfaceId: pending, agentId: "ag-new")

        #expect(registry.workspace(id: "ws-ag-a")?.agentIds == ["ag-new"])
    }

    @Test func closingAWorkspaceKillsItsAgentsAndRemovesFileOnlyWorkspacesToo() {
        let registry = makeRegistry(root: root, agents: ["ag-a"])
        registry.newTab(workspaceId: "ws-ag-a", leafId: 0, content: .file(path: nil))
        var killed: [String] = []
        registry.onCloseAgent = { _, agentId in killed.append(agentId) }

        registry.closeWorkspace("ws-ag-a")

        #expect(killed == ["ag-a"])
        #expect(registry.workspace(id: "ws-ag-a") == nil)
    }

    @Test func fileTabSessionsLiveUntilTheirTabCloses() {
        let registry = makeRegistry(root: root, agents: ["ag-a"])
        let fileTab = registry.newTab(workspaceId: "ws-ag-a", leafId: 0, content: .file(path: nil))!
        let session = registry.fileTabs.session(workspaceId: "ws-ag-a", surfaceId: fileTab, initialPath: nil)

        registry.selectTab(workspaceId: "ws-ag-a", surfaceId: 0)
        #expect(registry.fileTabs.existingSession(workspaceId: "ws-ag-a", surfaceId: fileTab) === session)

        registry.closeTab(workspaceId: "ws-ag-a", surfaceId: fileTab)
        #expect(registry.fileTabs.existingSession(workspaceId: "ws-ag-a", surfaceId: fileTab) == nil)
    }

    @Test func activatingAWorkspaceRecordsItInTheSavedLayout() throws {
        let registry = makeRegistry(root: root, agents: ["ag-a", "ag-b"])
        registry.activeWorkspaceId = "ws-ag-b"
        registry.saveAll()

        let data = try Data(contentsOf: URL(fileURLWithPath: WorkspacePersistence.filePath(projectRoot: root)))
        let document = try JSONDecoder().decode(PersistedWorkspaceDocument.self, from: data)
        #expect(document.activeWorkspaceId == "ws-ag-b")
        try? FileManager.default.removeItem(atPath: root)
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

    // MARK: - Layout written by another Mac

    /// Write `workspaces` as another Mac reaching this project over SSH would.
    private func writeLayoutFromAnotherMac(_ workspaces: [Workspace], projectRoot: String) throws {
        let path = WorkspacePersistence.filePath(projectRoot: projectRoot)
        try FileManager.default.createDirectory(
            atPath: (path as NSString).deletingLastPathComponent, withIntermediateDirectories: true)
        let data = try #require(WorkspacePersistence.documentData(workspaces, activeWorkspaceId: nil))
        // A distinct modification time, as a write from another process would have.
        Thread.sleep(forTimeInterval: 0.01)
        try data.write(to: URL(fileURLWithPath: path), options: .atomic)
    }

    @Test func givenAnotherMacSplitAPaneShouldPlaceItsNewAgentInThatPane() throws {
        let project = root + "-split"
        defer { try? FileManager.default.removeItem(atPath: project) }
        let registry = makeRegistry(root: project, agents: ["ag-a"])
        var theirs = try #require(registry.workspace(id: "ws-ag-a"))
        theirs.split(leafId: 0, axis: .vertical)
        let tab = try #require(theirs.focusedSurface).id
        try writeLayoutFromAnotherMac([theirs], projectRoot: project)

        var spawned = agent("ag-new")
        spawned.placement = SpawnPlacement(workspace: "ws-ag-a", surface: tab)
        registry.reconcile(projectRoot: project, rootAgents: [agent("ag-a"), spawned], worktrees: [])

        #expect(registry.workspaces(forProject: project).map(\.id) == ["ws-ag-a"])
        #expect(registry.workspace(id: "ws-ag-a")?.surface(id: tab)?.content == .agent("ag-new"))
    }

    @Test func givenAnotherMacWroteTheLayoutASaveShouldAdoptItNotOverwriteIt() throws {
        let project = root + "-save"
        defer { try? FileManager.default.removeItem(atPath: project) }
        let registry = makeRegistry(root: project, agents: ["ag-a"])
        var theirs = try #require(registry.workspace(id: "ws-ag-a"))
        theirs.split(leafId: 0, axis: .vertical)
        try writeLayoutFromAnotherMac([theirs], projectRoot: project)
        let written = try Data(contentsOf: URL(fileURLWithPath: WorkspacePersistence.filePath(projectRoot: project)))

        registry.saveAll()

        #expect(registry.workspace(id: "ws-ag-a")?.paneCount == 2)
        let onDisk = try Data(contentsOf: URL(fileURLWithPath: WorkspacePersistence.filePath(projectRoot: project)))
        #expect(onDisk == written)
    }

    @Test func givenLayoutFileDeletedASaveShouldWriteItAgain() throws {
        let project = root + "-deleted"
        defer { try? FileManager.default.removeItem(atPath: project) }
        let registry = makeRegistry(root: project, agents: ["ag-a"])
        registry.saveAll()
        let path = WorkspacePersistence.filePath(projectRoot: project)
        try FileManager.default.removeItem(atPath: path)
        registry.split(workspaceId: "ws-ag-a", leafId: 0, axis: .vertical)

        registry.saveAll()

        let restored = try WorkspacePersistence.loadForRegistry(projectRoot: project)
        #expect(restored.first { $0.id == "ws-ag-a" }?.paneCount == 2)
    }

    @Test func givenAnotherMacChangedADifferentWorkspaceASaveShouldKeepBothChanges() throws {
        let project = root + "-merge"
        defer { try? FileManager.default.removeItem(atPath: project) }
        let registry = makeRegistry(root: project, agents: ["ag-a", "ag-b"])
        registry.saveAll()
        var theirs = registry.workspaces(forProject: project)
        let b = try #require(theirs.firstIndex { $0.id == "ws-ag-b" })
        theirs[b].split(leafId: 0, axis: .vertical)
        try writeLayoutFromAnotherMac(theirs, projectRoot: project)
        registry.split(workspaceId: "ws-ag-a", leafId: 0, axis: .vertical)

        registry.saveAll()  // conflicts, merges
        registry.saveAll()  // writes the merged layout

        #expect(registry.workspace(id: "ws-ag-a")?.paneCount == 2)
        #expect(registry.workspace(id: "ws-ag-b")?.paneCount == 2)
        let onDisk = try WorkspacePersistence.loadForRegistry(projectRoot: project)
        #expect(onDisk.map(\.paneCount) == [2, 2])
    }

    @Test func givenBothMacsChangedTheSameWorkspaceOneSaveShouldKeepThisMacsNewTabOnDisk() throws {
        let project = root + "-same"
        defer { try? FileManager.default.removeItem(atPath: project) }
        let registry = makeRegistry(root: project, agents: ["ag-a"])
        registry.saveAll()
        var theirs = registry.workspaces(forProject: project)
        theirs[0].split(leafId: 0, axis: .horizontal)
        try writeLayoutFromAnotherMac(theirs, projectRoot: project)
        registry.split(workspaceId: "ws-ag-a", leafId: 0, axis: .vertical)
        let tab = try #require(registry.workspace(id: "ws-ag-a")?.focusedSurface).id

        registry.saveNow(projectRoot: project)  // as a spawn does: conflicts, merges, writes

        #expect(registry.workspace(id: "ws-ag-a")?.surface(id: tab)?.content == .empty)
        let onDisk = try WorkspacePersistence.loadForRegistry(projectRoot: project)
        #expect(onDisk.first?.surface(id: tab)?.content == .empty)
        #expect(!registry.hasPendingSave(projectRoot: project))
    }

    @Test func givenAnotherMacsLayoutNamesAnAgentThisMacHasNotSeenYetItsTabShouldWaitForTheManifest() throws {
        let project = root + "-unseen"
        defer { try? FileManager.default.removeItem(atPath: project) }
        let registry = makeRegistry(root: project, agents: ["ag-a"])
        registry.saveAll()
        var theirs = try #require(registry.workspace(id: "ws-ag-a"))
        theirs.split(leafId: 0, axis: .vertical, content: .agent("ag-new"))
        try writeLayoutFromAnotherMac([theirs], projectRoot: project)

        registry.adoptLocalLayoutIfChanged(projectRoot: project)  // the file watcher, before the manifest

        #expect(registry.workspace(id: "ws-ag-a")?.agentIds == ["ag-a", "ag-new"])
        registry.reconcile(projectRoot: project, rootAgents: [agent("ag-a"), agent("ag-new")], worktrees: [])
        #expect(registry.workspace(id: "ws-ag-a")?.agentIds == ["ag-a", "ag-new"])
        // A manifest without it means the agent exited: its tab goes.
        registry.reconcile(projectRoot: project, rootAgents: [agent("ag-a")], worktrees: [])
        #expect(registry.workspace(id: "ws-ag-a")?.agentIds == ["ag-a"])
    }

    @Test func givenAgentSpawnedIntoPointGuardOnAnotherMacShouldLandInThisMacsPointGuard() throws {
        let project = root + "-pg"
        defer { try? FileManager.default.removeItem(atPath: project) }
        let registry = makeRegistry(root: project, agents: [])
        let pointGuard = try registry.ensurePointGuardWorkspace(projectRoot: project)
        registry.split(workspaceId: pointGuard, leafId: 0, axis: .vertical)
        let tab = try #require(registry.workspace(id: pointGuard)?.focusedSurface).id

        var spawned = agent("ag-new")
        spawned.placement = SpawnPlacement(workspace: "ws-point-guard:machine:other:" + project, surface: tab)
        registry.reconcile(projectRoot: project, rootAgents: [spawned], worktrees: [])

        #expect(registry.workspace(id: pointGuard)?.surface(id: tab)?.content == .agent("ag-new"))
        #expect(registry.workspaceId(forAgent: "ag-new") == pointGuard)
    }

    @Test func givenOwnSavesShouldNotBeMistakenForAnotherMacs() throws {
        let project = root + "-own"
        defer { try? FileManager.default.removeItem(atPath: project) }
        let registry = makeRegistry(root: project, agents: ["ag-a"])
        registry.saveAll()
        registry.split(workspaceId: "ws-ag-a", leafId: 0, axis: .vertical)

        registry.adoptLocalLayoutIfChanged(projectRoot: project)

        #expect(registry.workspace(id: "ws-ag-a")?.paneCount == 2)
    }

    // MARK: - Remote commands

    @Test func remoteSplitAndCloseIgnoreUnknownLeaves() {
        let registry = makeRegistry(root: root, agents: ["ag-a"])
        registry.activeWorkspaceId = "ws-ag-a"
        let before = registry.workspace(id: "ws-ag-a")

        registry.handleRemoteCommand(GridCommandPayload(.split(leafId: 99, axis: "v")), from: root)
        registry.handleRemoteCommand(GridCommandPayload(.close(leafId: 99)), from: root)

        #expect(registry.workspace(id: "ws-ag-a") == before)
    }

    @Test func remoteSplitWithoutLeafUsesFocusedLeaf() {
        let registry = makeRegistry(root: root, agents: ["ag-a"])
        registry.activeWorkspaceId = "ws-ag-a"

        registry.handleRemoteCommand(GridCommandPayload(.split(leafId: nil, axis: "v")), from: root)

        #expect(registry.workspace(id: "ws-ag-a")?.paneCount == 2)
    }

    @Test func remoteNewTabWithAgentOpensItBesideTheActiveTab() {
        let registry = makeRegistry(root: root, agents: ["ag-a", "ag-b"])
        registry.activeWorkspaceId = "ws-ag-a"

        registry.handleRemoteCommand(GridCommandPayload(.newTab(leafId: nil, agentId: "ag-b")), from: root)

        let workspace = registry.workspace(id: "ws-ag-a")
        #expect(workspace?.panes[0]?.tabs.map(\.content) == [.agent("ag-a"), .agent("ag-b")])
        #expect(workspace?.focusedAgentId == "ag-b")
        #expect(registry.workspace(id: "ws-ag-b") == nil)
    }

    @Test func remoteSelectTabUsesOneBasedPositionsAndCycles() {
        let registry = makeRegistry(root: root, agents: ["ag-a"])
        registry.activeWorkspaceId = "ws-ag-a"
        registry.newTab(workspaceId: "ws-ag-a", leafId: 0)

        registry.handleRemoteCommand(GridCommandPayload(.selectTab(leafId: nil, index: 1, direction: nil)), from: root)
        #expect(registry.workspace(id: "ws-ag-a")?.focusedAgentId == "ag-a")

        registry.handleRemoteCommand(GridCommandPayload(.selectTab(leafId: nil, index: nil, direction: "prev")), from: root)
        #expect(registry.workspace(id: "ws-ag-a")?.focusedSurface?.content == .empty)
    }

    @Test func remoteSetAgentWithoutLeafTargetsTheFocusedPanesActiveTab() {
        let registry = makeRegistry(root: root, agents: ["ag-a", "ag-b"])
        registry.activeWorkspaceId = "ws-ag-a"
        registry.newTab(workspaceId: "ws-ag-a", leafId: 0)

        registry.handleRemoteCommand(GridCommandPayload(.setAgent(leafId: nil, agentId: "ag-b")), from: root)

        #expect(registry.workspace(id: "ws-ag-a")?.agentIds == ["ag-a", "ag-b"])
    }

    @Test func remoteCommandNamingAWorkspaceTargetsItNotTheActiveOne() {
        let registry = makeRegistry(root: root, agents: ["ag-a", "ag-b"])
        registry.activeWorkspaceId = "ws-ag-a"
        var killed: [String] = []
        registry.onCloseAgent = { _, agentId in killed.append(agentId) }

        registry.handleRemoteCommand(
            GridCommandPayload(.closeTab(leafId: nil, tabId: 0), workspaceId: "ws-ag-b"), from: root)

        #expect(killed == ["ag-b"])
        #expect(registry.workspace(id: "ws-ag-a") != nil)
    }

    @Test func remoteCommandNamingAWorkspaceOfAnotherProjectIsIgnored() {
        let registry = makeRegistry(root: root, agents: ["ag-a"])
        registry.handleRemoteCommand(
            GridCommandPayload(.closeTab(leafId: nil, tabId: 0), workspaceId: "ws-ag-a"), from: root + "-other")
        #expect(registry.workspace(id: "ws-ag-a") != nil)
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
