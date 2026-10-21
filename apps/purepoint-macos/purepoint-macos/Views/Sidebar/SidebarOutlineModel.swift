#if os(macOS)
import AppKit
#else
import UIKit
#endif

// The sidebar outline's platform-free model, shared by the Mac `NSOutlineView` controller and
// its iPad `UICollectionView` twin: what the tree holds, which rows are open and what a row's
// selection means. Each platform only draws it. Menus live in `SidebarMenuModel.swift`.

// MARK: - Render state

/// Everything the rows show, compared before a rebuild so an unchanged refresh costs nothing.
struct SidebarRenderState: Equatable {
    let activeWorkspaceId: String?
    let projects: [ProjectRenderState]
    let machines: [MachineRenderState]

    struct MachineRenderState: Equatable {
        let id: UUID
        let name: String
        let state: MachineConnectionState
        let statusText: String
        let actionError: String?
        let isStartingDaemon: Bool
        let hiddenRevision: Int
        let projects: [ProjectRenderState]
        let hasLoadedProjects: Bool
    }

    struct ProjectRenderState: Equatable {
        let channelUnread: Int
        let isUnavailable: Bool
        let layoutError: String?
        let projectRoot: String
        let rootWorkspaces: [WorkspaceRenderState]
        let worktrees: [WorktreeRenderState]
        /// A project whose snapshot has not been read shows no worktrees yet, which is not a
        /// change to its rows — but it is a change to whether the tree may be pruned, so it
        /// has to reach the render state or the prune would be skipped.
        let hasLoadedWorktrees: Bool
    }

    struct WorktreeRenderState: Equatable {
        let id: String
        let branch: String
        let workspaces: [WorkspaceRenderState]
    }

    struct WorkspaceRenderState: Equatable {
        let id: String
        let title: String
        let paneCount: Int
        let tabCount: Int
        /// `nil` for a workspace holding only file tabs — it has no agent to report on.
        let status: AgentStatus?
        let signal: AgentSignalPresentation?
    }
}

@MainActor
extension SidebarRenderState {
    static func make(
        projects: [ProjectState], machines: [MachineState], workspacesByProject: [String: [Workspace]],
        activeWorkspaceId: String?
    ) -> SidebarRenderState {
        let render = { (project: ProjectState) in
            projectRenderState(project, workspaces: workspacesByProject[project.projectId] ?? [])
        }
        let machineStates = machines.map { machine in
            MachineRenderState(
                id: machine.machine.id, name: machine.machine.name, state: machine.state,
                statusText: machine.statusText, actionError: machine.actionError,
                isStartingDaemon: machine.isStartingDaemon, hiddenRevision: machine.hiddenCountRevision,
                projects: machine.projects.map(render),
                hasLoadedProjects: machine.hasLoadedProjects)
        }
        return SidebarRenderState(
            activeWorkspaceId: activeWorkspaceId, projects: projects.map(render), machines: machineStates)
    }

    /// How a workspace presents itself as one row: named after its first pane's agent,
    /// with the pane count when it holds more than one.
    static func workspace(_ workspace: Workspace, in project: ProjectState) -> WorkspaceRenderState {
        let agents = workspace.agentIds.compactMap { project.agent(byId: $0) }
        let firstFile = workspace.surfaces.lazy.compactMap { entry -> String? in
            guard case .file(let path) = entry.surface.content else { return nil }
            return path.map { ($0 as NSString).lastPathComponent } ?? "Files"
        }.first
        let title = agents.first?.displayName ?? workspace.primaryAgentId ?? firstFile ?? workspace.id
        let status: AgentStatus? =
            agents.isEmpty
            ? nil : agents.contains { !$0.status.isAlive } ? .broken : (agents.first?.status ?? .running)
        return WorkspaceRenderState(
            id: workspace.id, title: title, paneCount: workspace.paneCount, tabCount: workspace.tabCount,
            status: status, signal: agents.first.map { $0.signalPresentation(available: !project.isUnavailable) })
    }

    private static func projectRenderState(_ project: ProjectState, workspaces: [Workspace]) -> ProjectRenderState {
        let rootWorkspaces =
            workspaces
            .filter { $0.container == .projectRoot && !$0.isPointGuard }
            .map { workspace($0, in: project) }

        let worktrees = project.worktrees.map { worktree in
            WorktreeRenderState(
                id: worktree.id,
                branch: worktree.branch,
                workspaces:
                    workspaces
                    .filter { $0.container == .worktree(worktree.id) }
                    .map { workspace($0, in: project) }
            )
        }

        return ProjectRenderState(
            channelUnread: project.channel.unreadCount,
            isUnavailable: project.isUnavailable,
            layoutError: project.isRemote ? project.registry?.layoutErrorsByProject[project.projectId] : nil,
            projectRoot: project.projectId,
            rootWorkspaces: rootWorkspaces,
            worktrees: worktrees,
            hasLoadedWorktrees: project.hasLoadedWorktrees
        )
    }
}

// MARK: - Tree

@MainActor
enum SidebarTree {
    /// The local projects, then a MACHINES section with each machine's projects nested under it.
    static func build(
        projects: [ProjectState], machines: [MachineState], workspacesByProject: [String: [Workspace]]
    ) -> [SidebarNode] {
        var nodes = projects.map { SidebarNode.projectNode($0, workspaces: workspacesByProject[$0.projectId] ?? []) }
        if !machines.isEmpty {
            nodes.append(SidebarNode.sectionHeaderNode("MACHINES"))
            for machine in machines {
                let machineKey = SidebarNode.machineKey(machine.machine.id)
                nodes.append(
                    SidebarNode(
                        kind: .machine(machine),
                        rowKey: machineKey,
                        children: machine.projects.map {
                            SidebarNode.projectNode(
                                $0, workspaces: workspacesByProject[$0.projectId] ?? [], parentKey: machineKey)
                        }))
            }
        }
        return nodes
    }

    /// Whether every row in the tree holds all the rows it will ever hold. `false` while a
    /// project has yet to read its snapshot and a machine has yet to answer its inventory,
    /// which is how the sidebar looks for the first moments after a relaunch or a machine
    /// connection.
    ///
    /// An empty row in that window is not evidence that nothing is under it, so nothing may
    /// be pruned on that basis. A remote project is registered with the registry before its
    /// snapshot has been read, which is why the registry cannot answer this on its own; and a
    /// machine whose daemon is not running cannot answer for its rows either.
    static func isSettled(_ nodes: [SidebarNode]) -> Bool {
        nodes.allSatisfy { node in
            switch node.kind {
            case .project(let project) where !project.hasLoadedWorktrees:
                return false
            case .machine(let machine) where !machine.reportsCompleteProjects:
                return false
            case .project, .machine:
                return isSettled(node.children)
            case .worktree, .workspace, .sectionHeader:
                return true
            }
        }
    }

    /// Which rows to open and which are closed, given the rows the user closed.
    ///
    /// `open` lists the rows on screen that are not recorded closed, parents before children,
    /// in the order they are to be expanded. `closed` is every recorded-closed row still in the
    /// tree. A row inside a closed row has no row of its own, so it is not opened, but it is
    /// still closed — that is what lets it come back as it was when its parent opens again.
    static func expansion(
        _ nodes: [SidebarNode], closedRowKeys: Set<String>, parentOpen: Bool = true
    ) -> (open: [SidebarNode], closed: Set<String>) {
        var open: [SidebarNode] = []
        var closed: Set<String> = []
        for node in nodes where node.isExpandable {
            let isClosed = closedRowKeys.contains(node.rowKey)
            if isClosed {
                closed.insert(node.rowKey)
            } else if parentOpen {
                open.append(node)
            }
            let below = expansion(node.children, closedRowKeys: closedRowKeys, parentOpen: parentOpen && !isClosed)
            open += below.open
            closed.formUnion(below.closed)
        }
        return (open, closed)
    }

    /// The rows above a matching row, outermost first. Empty when the row is at the top level
    /// or is not in the tree.
    static func ancestors(where matches: (SidebarNode) -> Bool, in nodes: [SidebarNode]) -> [SidebarNode] {
        func find(in nodes: [SidebarNode], chain: [SidebarNode]) -> [SidebarNode]? {
            for node in nodes {
                if matches(node) {
                    return chain
                }
                if let found = find(in: node.children, chain: chain + [node]) {
                    return found
                }
            }
            return nil
        }
        // `nil` is no match at all, which a caller reads as "no ancestors".
        return find(in: nodes, chain: []) ?? []
    }

    /// Local projects and every machine's projects, in sidebar order.
    static func allProjects(in nodes: [SidebarNode]) -> [ProjectState] {
        nodes.flatMap { node -> [ProjectState] in
            switch node.kind {
            case .project(let p): return [p]
            case .machine: return node.children.compactMap { if case .project(let p) = $0.kind { p } else { nil } }
            default: return []
            }
        }
    }

    /// The first worktree row with this id.
    static func worktreeNode(id: String, in nodes: [SidebarNode]) -> SidebarNode? {
        for node in nodes {
            if case .worktree(let worktree) = node.kind, worktree.id == id { return node }
            if let found = worktreeNode(id: id, in: node.children) { return found }
        }
        return nil
    }

    /// Section titles cannot be selected. A machine row can — so keyboard and VoiceOver users
    /// can reach its actions (Return toggles the connection; the context menu and accessibility
    /// actions hold the rest) — but it addresses nothing in the detail view.
    static func isSelectable(_ node: SidebarNode) -> Bool {
        switch node.kind {
        case .sectionHeader: return false
        case .project, .worktree, .workspace, .machine: return true
        }
    }

    /// What selecting a row tells the rest of the app; `nil` for rows that report nothing.
    static func selection(for node: SidebarNode) -> SidebarSelection? {
        switch node.kind {
        case .project(let p): return .project(p.projectId)
        case .worktree(let w): return .worktree(w.id)
        case .workspace(let w): return .workspace(w.id)
        case .sectionHeader, .machine: return nil
        }
    }

    /// The row id a selection addresses; `nil` means no row should be selected.
    static func targetId(for selection: SidebarSelection?) -> String? {
        switch selection {
        case .workspace(let id), .worktree(let id), .channel(let id), .project(let id): return id
        case .nav, nil: return nil
        }
    }
}

// MARK: - Machine status

extension MachineConnectionState {
    /// The machine row's status dot.
    var sidebarStatusColor: NSColor {
        switch self {
        case .connected(.running): .systemGreen
        case .connected(.stopped): .systemOrange
        case .connected(.incompatible): .systemRed
        case .connecting: .systemYellow
        case .offline: .systemRed
        case .disconnected: .tertiaryLabelColor
        }
    }
}
