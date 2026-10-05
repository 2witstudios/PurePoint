import Foundation
import Observation

/// A tab that has asked the daemon for an agent and is waiting for it to appear.
///
/// Reservations are what make spawning deterministic. The tab records its claim *before*
/// the daemon writes the manifest, so when the new agent shows up the reconciler places it
/// in that tab directly. It is never briefly adopted into a workspace of its own, which is
/// how a freshly split pane used to flash as a second sidebar row.
private struct SurfaceReservation {
    let workspaceId: String
    let surfaceId: Int
    let container: WorkspaceContainer
}

/// The single source of truth for pane grouping across every open project.
///
/// The sidebar, the detail view, and the menu commands all read workspaces from here and
/// nowhere else. There is no second list of agents to filter, hide, or subtract — the row
/// set *is* `workspaces(forProject:)`, so a pane and a loose sidebar item for the same agent
/// cannot coexist. `reconcile` is the only writer of that list.
@Observable
@MainActor
final class WorkspaceRegistry {

    /// Canonical workspaces per project root, in sidebar order.
    private(set) var workspacesByProject: [String: [Workspace]] = [:]

    /// The workspace whose grid is on screen.
    var activeWorkspaceId: String?

    /// Set after a UI-initiated split or new tab so that empty tab opens the command palette.
    var pendingPaletteSurfaceId: Int?

    /// Invoked with (projectRoot, agentId) when a tab holding an agent is closed.
    @ObservationIgnored var onCloseAgent: ((String, String) -> Void)?

    @ObservationIgnored private var reservationsByProject: [String: [SurfaceReservation]] = [:]
    /// The latest live-agent list each project's manifest produced, kept so a pane binding
    /// can reconcile immediately instead of waiting for the next manifest change.
    @ObservationIgnored private var liveByProject: [String: [LiveAgent]] = [:]
    @ObservationIgnored private var loadedProjects = Set<String>()
    @ObservationIgnored private var saveWorkItems: [String: DispatchWorkItem] = [:]

    // MARK: - Queries

    func workspaces(forProject projectRoot: String) -> [Workspace] {
        workspacesByProject[projectRoot] ?? []
    }

    func workspace(id: String) -> Workspace? {
        for (_, list) in workspacesByProject {
            if let match = list.first(where: { $0.id == id }) { return match }
        }
        return nil
    }

    func projectRoot(forWorkspace id: String) -> String? {
        for (root, list) in workspacesByProject where list.contains(where: { $0.id == id }) {
            return root
        }
        return nil
    }

    /// The workspace holding an agent. Exactly one does, by construction.
    func workspaceId(forAgent agentId: String) -> String? {
        for (_, list) in workspacesByProject {
            if let match = list.first(where: { $0.contains(agentId: agentId) }) { return match.id }
        }
        return nil
    }

    var activeWorkspace: Workspace? {
        activeWorkspaceId.flatMap { workspace(id: $0) }
    }

    // MARK: - Reconcile

    /// Fold a project's manifest into the stored layout and publish the canonical result.
    ///
    /// Called on every manifest change and status push. Loading the layout from disk happens
    /// here on first sight of a project — the old code persisted a layout that nothing ever
    /// read back, so every restart silently dissolved the grouping.
    func reconcile(projectRoot: String, rootAgents: [AgentModel], worktrees: [WorktreeModel]) {
        if loadedProjects.insert(projectRoot).inserted, workspacesByProject[projectRoot] == nil {
            workspacesByProject[projectRoot] = WorkspacePersistence.load(projectRoot: projectRoot)
        }

        let live = WorkspaceReconciler.liveAgents(rootAgents: rootAgents, worktrees: worktrees)
        liveByProject[projectRoot] = live
        publish(projectRoot: projectRoot, stored: workspacesByProject[projectRoot] ?? [], live: live)
    }

    /// Reconcile `stored` against `live` and publish the result if it changed.
    private func publish(projectRoot: String, stored: [Workspace], live: [LiveAgent]) {
        let reconciled = WorkspaceReconciler.reconcile(
            stored: stored, live: deferringReservedAgents(projectRoot: projectRoot, stored: stored, live: live))
        guard reconciled != workspacesByProject[projectRoot] else { return }

        workspacesByProject[projectRoot] = reconciled
        pruneActiveSelection()
        scheduleSave(projectRoot: projectRoot)
    }

    /// Hold back agents that no tab has claimed while a tab in their container is waiting
    /// for its spawn response. Which of them belongs to the reservation is only known once the
    /// response binds an ID, so adopting any of them now could hand the pane to an unrelated
    /// agent or surface the real one as a workspace of its own.
    private func deferringReservedAgents(
        projectRoot: String, stored: [Workspace], live: [LiveAgent]
    ) -> [LiveAgent] {
        guard let reservations = reservationsByProject[projectRoot], !reservations.isEmpty else { return live }
        let reservedContainers = Set(reservations.map(\.container))
        let placed = Set(stored.flatMap(\.agentIds))
        return live.filter { placed.contains($0.id) || !reservedContainers.contains($0.container) }
    }

    /// Drop the active selection if its workspace no longer exists.
    private func pruneActiveSelection() {
        if let id = activeWorkspaceId, workspace(id: id) == nil {
            activeWorkspaceId = nil
        }
    }

    // MARK: - Reservations

    /// Claim a tab for the agent a spawn request is about to create.
    func reserveSurface(projectRoot: String, workspaceId: String, surfaceId: Int) {
        guard let target = workspace(id: workspaceId) else { return }
        reservationsByProject[projectRoot, default: []].append(
            SurfaceReservation(workspaceId: workspaceId, surfaceId: surfaceId, container: target.container)
        )
    }

    /// The spawn response arrived first — bind the agent to its tab immediately.
    func fulfillReservation(projectRoot: String, workspaceId: String, surfaceId: Int, agentId: String) {
        dropReservation(projectRoot: projectRoot, workspaceId: workspaceId, surfaceId: surfaceId)
        setAgent(agentId, workspaceId: workspaceId, surfaceId: surfaceId)
    }

    /// The spawn failed or completed — stop holding the tab, and let any agents that were
    /// held back behind the reservation surface.
    func releaseReservation(projectRoot: String, workspaceId: String, surfaceId: Int) {
        dropReservation(projectRoot: projectRoot, workspaceId: workspaceId, surfaceId: surfaceId)
        if let live = liveByProject[projectRoot] {
            publish(projectRoot: projectRoot, stored: workspacesByProject[projectRoot] ?? [], live: live)
        }
    }

    private func dropReservation(projectRoot: String, workspaceId: String, surfaceId: Int) {
        guard var reservations = reservationsByProject[projectRoot] else { return }
        if let index = reservations.firstIndex(where: { $0.workspaceId == workspaceId && $0.surfaceId == surfaceId }) {
            reservations.remove(at: index)
        }
        reservationsByProject[projectRoot] = reservations.isEmpty ? nil : reservations
    }

    // MARK: - Pane Mutations

    func split(workspaceId: String, leafId: Int, axis: PaneSplitNode.Axis) {
        mutate(workspaceId) { $0.split(leafId: leafId, axis: axis) }
    }

    /// Close a pane and every tab in it, killing the agents they held.
    func closePane(workspaceId: String, leafId: Int) {
        guard let workspace = workspace(id: workspaceId), let pane = workspace.panes[leafId] else { return }
        let surfaceIds = pane.tabs.map(\.id)
        removeTabs(workspaceId: workspaceId, surfaceIds: surfaceIds, dropsWorkspace: workspace.paneCount <= 1) {
            $0.closePane(leafId: leafId)
        }
    }

    func setRatio(_ ratio: CGFloat, workspaceId: String, forSplitIdentifiedByFirstLeaf leafId: Int) {
        mutate(workspaceId) { $0.setRatio(ratio, forSplitIdentifiedByFirstLeaf: leafId) }
    }

    func setFocus(workspaceId: String, leafId: Int) {
        mutate(workspaceId) { workspace in
            guard workspace.root.allLeafIds.contains(leafId) else { return }
            workspace.focusedLeafId = leafId
        }
    }

    func moveFocus(workspaceId: String, direction: FocusDirection) {
        mutate(workspaceId) { $0.moveFocus(direction: direction) }
    }

    /// When the focused pane is not showing an agent, focus the first pane that is. Used when
    /// the user clicks a sidebar row: a single-pane workspace opens straight into its terminal
    /// with no second click. Which tab each pane shows is left alone.
    func activate(workspaceId: String) {
        activeWorkspaceId = workspaceId
        guard let target = workspace(id: workspaceId), target.focusedAgentId == nil,
            let leafId = target.root.allLeafIds.first(where: { target.panes[$0]?.activeTab?.content.agentId != nil })
        else { return }
        setFocus(workspaceId: workspaceId, leafId: leafId)
    }

    // MARK: - Tab Mutations

    /// Open a tab after the pane's active one. Returns the new tab's ID.
    @discardableResult
    func newTab(workspaceId: String, leafId: Int, content: SurfaceContent = .empty) -> Int? {
        var surfaceId: Int?
        mutate(workspaceId) { surfaceId = $0.newTab(leafId: leafId, content: content) }
        return surfaceId
    }

    func selectTab(workspaceId: String, surfaceId: Int) {
        mutate(workspaceId) { $0.selectTab(surfaceId) }
    }

    /// Select by 0-based position in a pane.
    func selectTab(workspaceId: String, leafId: Int, index: Int) {
        mutate(workspaceId) { $0.selectTab(leafId: leafId, index: index) }
    }

    func selectLastTab(workspaceId: String, leafId: Int) {
        mutate(workspaceId) { $0.selectLastTab(leafId: leafId) }
    }

    func cycleTab(workspaceId: String, leafId: Int, by offset: Int) {
        mutate(workspaceId) { $0.cycleTab(leafId: leafId, by: offset) }
    }

    /// Close a tab, killing the agent it held. Its pane goes with its last tab, and the
    /// workspace goes with its last pane, so neither lingers as an empty shell.
    func closeTab(workspaceId: String, surfaceId: Int) {
        guard let workspace = workspace(id: workspaceId), workspace.leafId(ofSurface: surfaceId) != nil else { return }
        removeTabs(workspaceId: workspaceId, surfaceIds: [surfaceId], dropsWorkspace: workspace.isLastTab(surfaceId)) {
            $0.closeTab(surfaceId)
        }
    }

    /// Move a tab to a 0-based position in a pane (`nil` appends). The agent keeps running.
    func moveTab(workspaceId: String, surfaceId: Int, toLeaf leafId: Int, index: Int? = nil) {
        mutate(workspaceId) { $0.moveTab(surfaceId, toLeaf: leafId, index: index) }
    }

    func breakTab(workspaceId: String, surfaceId: Int, axis: PaneSplitNode.Axis) {
        mutate(workspaceId) { $0.breakTab(surfaceId, axis: axis) }
    }

    func joinPane(workspaceId: String, leafId: Int, into targetLeafId: Int) {
        mutate(workspaceId) { $0.joinPane(leafId, into: targetLeafId) }
    }

    /// Show the file navigator in a tab (optionally opened on a file). No daemon involved.
    func openFile(workspaceId: String, surfaceId: Int, path: String?) {
        mutate(workspaceId) { $0.setContent(.file(path: path), forSurface: surfaceId) }
    }

    /// Bind an agent to a tab. The binding wins over any other tab still claiming the same
    /// agent, and the project is reconciled straight away so selection and persistence see
    /// one consistent result.
    func setAgent(_ agentId: String?, workspaceId: String, surfaceId: Int) {
        guard let agentId else {
            mutate(workspaceId) { $0.setContent(.empty, forSurface: surfaceId) }
            return
        }
        guard let projectRoot = projectRoot(forWorkspace: workspaceId),
            let list = workspacesByProject[projectRoot],
            let target = list.first(where: { $0.id == workspaceId }),
            target.leafId(ofSurface: surfaceId) != nil
        else { return }

        var stored: [Workspace] = []
        for var workspace in list {
            if workspace.id == workspaceId {
                _ = workspace.removeSurfaces { $0.id != surfaceId && $0.content.agentId == agentId }
                workspace.setContent(.agent(agentId), forSurface: surfaceId)
            } else if workspace.contains(agentId: agentId) {
                guard workspace.removeSurfaces(where: { $0.content.agentId == agentId }) else { continue }
            }
            workspace.normalize()
            stored.append(workspace)
        }

        // The spawn response can beat the manifest; the agent is real even if the retained
        // list has not seen it yet, so it must not be pruned as dead.
        var live = liveByProject[projectRoot] ?? []
        if !live.contains(where: { $0.id == agentId }) {
            live.append(LiveAgent(id: agentId, container: target.container))
        }
        publish(projectRoot: projectRoot, stored: stored, live: live)
    }

    /// Shared path for closing tabs and panes: apply `body`, drop the workspace when nothing
    /// is left to show, release reservations on the removed tabs, and kill their agents.
    private func removeTabs(
        workspaceId: String, surfaceIds: [Int], dropsWorkspace: Bool, _ body: (inout Workspace) -> [String]
    ) {
        guard let projectRoot = projectRoot(forWorkspace: workspaceId),
            var list = workspacesByProject[projectRoot],
            let index = list.firstIndex(where: { $0.id == workspaceId })
        else { return }

        let killed = body(&list[index])
        if dropsWorkspace {
            list.remove(at: index)
            if activeWorkspaceId == workspaceId { activeWorkspaceId = nil }
        } else {
            list[index].normalize()
        }

        workspacesByProject[projectRoot] = list
        for surfaceId in surfaceIds {
            dropReservation(projectRoot: projectRoot, workspaceId: workspaceId, surfaceId: surfaceId)
        }
        scheduleSave(projectRoot: projectRoot)

        for agentId in killed {
            onCloseAgent?(projectRoot, agentId)
        }
    }

    private func mutate(_ workspaceId: String, _ body: (inout Workspace) -> Void) {
        guard let projectRoot = projectRoot(forWorkspace: workspaceId),
            var list = workspacesByProject[projectRoot],
            let index = list.firstIndex(where: { $0.id == workspaceId })
        else { return }

        let before = list[index]
        body(&list[index])
        list[index].normalize()
        // Writing observed state invalidates every view that reads it, so skip no-ops
        // (e.g. focusing the pane that is already focused).
        guard list[index] != before else { return }
        workspacesByProject[projectRoot] = list
        scheduleSave(projectRoot: projectRoot)
    }

    // MARK: - Remote Commands

    /// Grid commands from `pu grid ...` address panes and tabs but know nothing about
    /// workspaces, so they apply to whichever workspace is on screen for that project.
    /// Tab positions arrive 1-based (as `pu grid show` prints them); tab IDs are surface IDs.
    func handleRemoteCommand(_ command: GridCommandPayload, from sourceProjectRoot: String) {
        guard let workspaceId = activeWorkspaceId,
            projectRoot(forWorkspace: workspaceId) == sourceProjectRoot,
            let current = workspace(id: workspaceId)
        else { return }

        func pane(_ leafId: Int?) -> Int? {
            let target = leafId ?? current.focusedLeafId
            return current.panes[target] == nil ? nil : target
        }
        func axis(_ value: String) -> PaneSplitNode.Axis { value == "h" ? .horizontal : .vertical }

        switch command {
        case .split(let leafId, let axisStr):
            guard let target = pane(leafId) else { return }
            split(workspaceId: workspaceId, leafId: target, axis: axis(axisStr))
        case .close(let leafId):
            guard let target = pane(leafId) else { return }
            closePane(workspaceId: workspaceId, leafId: target)
        case .focus(let leafId, let directionStr):
            if let leafId {
                setFocus(workspaceId: workspaceId, leafId: leafId)
            } else if let directionStr {
                let direction: FocusDirection =
                    switch directionStr {
                    case "up": .up
                    case "down": .down
                    case "left": .left
                    default: .right
                    }
                moveFocus(workspaceId: workspaceId, direction: direction)
            }
        case .setAgent(let leafId, let agentId):
            guard let target = pane(leafId), let surfaceId = current.panes[target]?.activeTab?.id else { return }
            setAgent(agentId, workspaceId: workspaceId, surfaceId: surfaceId)
        case .newTab(let leafId, let agentId):
            guard let target = pane(leafId),
                let surfaceId = newTab(workspaceId: workspaceId, leafId: target)
            else { return }
            if let agentId { setAgent(agentId, workspaceId: workspaceId, surfaceId: surfaceId) }
        case .selectTab(let leafId, let index, let direction):
            guard let target = pane(leafId) else { return }
            if let index {
                selectTab(workspaceId: workspaceId, leafId: target, index: index - 1)
            } else if let direction {
                cycleTab(workspaceId: workspaceId, leafId: target, by: direction == "prev" ? -1 : 1)
            }
        case .closeTab(let leafId, let tabId):
            if let tabId {
                closeTab(workspaceId: workspaceId, surfaceId: tabId)
            } else if let target = pane(leafId), let surfaceId = current.panes[target]?.activeTab?.id {
                closeTab(workspaceId: workspaceId, surfaceId: surfaceId)
            }
        case .moveTab(let tabId, let toLeaf, let index):
            moveTab(workspaceId: workspaceId, surfaceId: tabId, toLeaf: toLeaf, index: index.map { $0 - 1 })
        case .breakTab(let tabId, let axisStr):
            guard let surfaceId = tabId ?? current.focusedSurface?.id else { return }
            breakTab(workspaceId: workspaceId, surfaceId: surfaceId, axis: axis(axisStr))
        case .getLayout:
            break  // daemon answers this from the file directly
        }
    }

    // MARK: - Persistence

    /// Debounced write (1 second coalesce).
    private func scheduleSave(projectRoot: String) {
        saveWorkItems[projectRoot]?.cancel()
        let snapshot = workspacesByProject[projectRoot] ?? []
        let item = DispatchWorkItem {
            WorkspacePersistence.save(snapshot, projectRoot: projectRoot)
        }
        saveWorkItems[projectRoot] = item
        DispatchQueue.main.asyncAfter(deadline: .now() + 1.0, execute: item)
    }

    /// Flush pending writes — called on quit.
    func saveAll() {
        for (projectRoot, workspaces) in workspacesByProject {
            saveWorkItems[projectRoot]?.cancel()
            WorkspacePersistence.save(workspaces, projectRoot: projectRoot)
        }
    }

    func forgetProject(_ projectRoot: String) {
        saveWorkItems[projectRoot]?.cancel()
        saveWorkItems[projectRoot] = nil
        workspacesByProject[projectRoot] = nil
        reservationsByProject[projectRoot] = nil
        liveByProject[projectRoot] = nil
        loadedProjects.remove(projectRoot)
        pruneActiveSelection()
    }
}
