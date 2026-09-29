import Foundation
import Observation

/// A pane that has asked the daemon for an agent and is waiting for it to appear.
///
/// Reservations are what make spawning deterministic. The pane records its claim *before*
/// the daemon writes the manifest, so when the new agent shows up the reconciler places it
/// in that pane directly. It is never briefly adopted into a workspace of its own, which is
/// how a freshly split pane used to flash as a second sidebar row.
private struct PaneReservation {
    let workspaceId: String
    let leafId: Int
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

    /// Set after a UI-initiated split so the new pane opens the command palette.
    var pendingPaletteLeafId: Int?

    /// Invoked with (projectRoot, agentId) when a pane holding an agent is closed.
    @ObservationIgnored var onCloseAgent: ((String, String) -> Void)?

    @ObservationIgnored private var reservationsByProject: [String: [PaneReservation]] = [:]
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

    /// Hold back agents that no pane has claimed while a pane in their container is waiting
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

    /// Claim a pane for the agent a spawn request is about to create.
    func reservePane(projectRoot: String, workspaceId: String, leafId: Int) {
        guard let target = workspace(id: workspaceId) else { return }
        reservationsByProject[projectRoot, default: []].append(
            PaneReservation(workspaceId: workspaceId, leafId: leafId, container: target.container)
        )
    }

    /// The spawn response arrived first — bind the agent to its pane immediately.
    func fulfillReservation(projectRoot: String, workspaceId: String, leafId: Int, agentId: String) {
        dropReservation(projectRoot: projectRoot, workspaceId: workspaceId, leafId: leafId)
        setAgent(agentId, workspaceId: workspaceId, leafId: leafId)
    }

    /// The spawn failed or completed — stop holding the pane, and let any agents that were
    /// held back behind the reservation surface.
    func releaseReservation(projectRoot: String, workspaceId: String, leafId: Int) {
        dropReservation(projectRoot: projectRoot, workspaceId: workspaceId, leafId: leafId)
        if let live = liveByProject[projectRoot] {
            publish(projectRoot: projectRoot, stored: workspacesByProject[projectRoot] ?? [], live: live)
        }
    }

    private func dropReservation(projectRoot: String, workspaceId: String, leafId: Int) {
        guard var reservations = reservationsByProject[projectRoot] else { return }
        if let index = reservations.firstIndex(where: { $0.workspaceId == workspaceId && $0.leafId == leafId }) {
            reservations.remove(at: index)
        }
        reservationsByProject[projectRoot] = reservations.isEmpty ? nil : reservations
    }

    // MARK: - Mutations

    func split(workspaceId: String, leafId: Int, axis: PaneSplitNode.Axis) {
        mutate(workspaceId) { $0.split(leafId: leafId, axis: axis) }
    }

    /// Close a pane. Kills the agent that occupied it; drops the workspace when its last
    /// pane goes, so a workspace never lingers as an empty row.
    func closePane(workspaceId: String, leafId: Int) {
        guard let projectRoot = projectRoot(forWorkspace: workspaceId),
            var list = workspacesByProject[projectRoot],
            let index = list.firstIndex(where: { $0.id == workspaceId })
        else { return }

        let isLastPane = list[index].paneCount <= 1
        let occupant = list[index].closePane(leafId: leafId)

        if isLastPane {
            list.remove(at: index)
            if activeWorkspaceId == workspaceId { activeWorkspaceId = nil }
        } else {
            list[index].normalize()
        }

        workspacesByProject[projectRoot] = list
        dropReservation(projectRoot: projectRoot, workspaceId: workspaceId, leafId: leafId)
        scheduleSave(projectRoot: projectRoot)

        if let occupant {
            onCloseAgent?(projectRoot, occupant)
        }
    }

    /// Bind an agent to a pane. The binding wins over any other pane still claiming the
    /// same agent, and the project is reconciled straight away so selection and persistence
    /// see one consistent result.
    func setAgent(_ agentId: String?, workspaceId: String, leafId: Int) {
        guard let agentId else {
            mutate(workspaceId) { $0.setAgent(nil, forLeafId: leafId) }
            return
        }
        guard let projectRoot = projectRoot(forWorkspace: workspaceId),
            let list = workspacesByProject[projectRoot],
            let target = list.first(where: { $0.id == workspaceId }),
            target.root.allLeafIds.contains(leafId)
        else { return }

        var stored: [Workspace] = []
        for var workspace in list {
            if workspace.id == workspaceId {
                workspace.setAgent(agentId, forLeafId: leafId)
            } else if workspace.contains(agentId: agentId) {
                let claimed = Set(workspace.root.leaves.filter { $0.agentId == agentId }.map(\.id))
                guard let pruned = workspace.root.removingLeaves(ids: claimed) else { continue }
                workspace.root = pruned
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

    /// Focus the workspace's first pane. Used when the user clicks a sidebar row: a
    /// single-pane workspace opens straight into its terminal with no second click.
    func activate(workspaceId: String) {
        activeWorkspaceId = workspaceId
        guard let target = workspace(id: workspaceId) else { return }
        if target.focusedAgentId == nil,
            let firstAgentLeaf = target.root.leaves.first(where: { $0.agentId != nil })
        {
            setFocus(workspaceId: workspaceId, leafId: firstAgentLeaf.id)
        }
    }

    private func mutate(_ workspaceId: String, _ body: (inout Workspace) -> Void) {
        guard let projectRoot = projectRoot(forWorkspace: workspaceId),
            var list = workspacesByProject[projectRoot],
            let index = list.firstIndex(where: { $0.id == workspaceId })
        else { return }

        body(&list[index])
        list[index].normalize()
        workspacesByProject[projectRoot] = list
        scheduleSave(projectRoot: projectRoot)
    }

    // MARK: - Remote Commands

    /// Grid commands from `pu grid ...` address panes but know nothing about workspaces,
    /// so they apply to whichever workspace is on screen for that project.
    func handleRemoteCommand(_ command: GridCommandPayload, from sourceProjectRoot: String) {
        guard let workspaceId = activeWorkspaceId,
            projectRoot(forWorkspace: workspaceId) == sourceProjectRoot,
            let current = workspace(id: workspaceId)
        else { return }

        switch command {
        case .split(let leafId, let axisStr):
            let target = leafId ?? current.focusedLeafId
            guard current.root.allLeafIds.contains(target) else { return }
            split(workspaceId: workspaceId, leafId: target, axis: axisStr == "h" ? .horizontal : .vertical)
        case .close(let leafId):
            let target = leafId ?? current.focusedLeafId
            guard current.root.allLeafIds.contains(target) else { return }
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
            setAgent(agentId, workspaceId: workspaceId, leafId: Int(leafId))
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
