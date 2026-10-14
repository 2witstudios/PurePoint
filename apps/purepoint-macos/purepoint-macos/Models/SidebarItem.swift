import SwiftUI

enum SidebarNavItem: String, CaseIterable, Identifiable {
    case dashboard
    case agents
    case schedule

    var id: String { rawValue }

    var title: String {
        rawValue.capitalized
    }

    var icon: String {
        switch self {
        case .dashboard: "bubble.left.and.bubble.right.fill"
        case .agents: "cpu"
        case .schedule: "calendar"
        }
    }
}

/// What the sidebar can have selected.
///
/// Note there is no `.agent` case. Agents are not addressable from the sidebar — they live
/// in panes, and panes live in workspaces. Removing the case is what makes it impossible for
/// a pane to reappear as its own row: there is no representation for such a row.
enum SidebarSelection: Hashable {
    case nav(SidebarNavItem)
    case workspace(String)
    case worktree(String)
    case channel(String)  // ProjectKey.id
    case project(String)  // ProjectKey.id — the root path for local projects
}

// MARK: - SidebarNode (NSOutlineView reference-type wrapper)

class SidebarNode {
    enum Kind {
        case project(ProjectState)
        case worktree(WorktreeModel)
        case workspace(Workspace)
        /// The "MACHINES" section title below the local projects.
        case sectionHeader(String)
        case machine(MachineState)
    }

    let kind: Kind
    var children: [SidebarNode] = []

    /// Row identity: every ancestor's scope, then this row's own kind and id. `id` alone is
    /// not enough — a worktree id under two projects, or the same project root under two
    /// machines, would otherwise be one row as far as expansion is concerned.
    let rowKey: String

    init(kind: Kind, rowKey: String, children: [SidebarNode] = []) {
        self.kind = kind
        self.rowKey = rowKey
        self.children = children
    }

    /// What a selection addresses: the project key, worktree id or workspace id the rest of
    /// the app knows the row by.
    var id: String {
        switch kind {
        case .project(let p): return p.projectId
        case .worktree(let w): return w.id
        case .workspace(let w): return w.id
        case .sectionHeader(let title): return "section:" + title
        case .machine(let m): return "machine-node:" + m.machine.id.uuidString.lowercased()
        }
    }

    var isExpandable: Bool {
        switch kind {
        case .project, .worktree, .machine: return true
        case .workspace, .sectionHeader: return false
        }
    }
}

// MARK: - Tree building

extension SidebarNode {
    /// A project row with its subtree: the project root's workspaces first, then one row
    /// per worktree holding that worktree's workspaces.
    static func projectNode(_ project: ProjectState, workspaces: [Workspace], parentKey: String? = nil) -> SidebarNode {
        let projectKey = rowKey("project", project.projectId, under: parentKey)
        let rootWorkspaces =
            workspaces
            .filter { $0.container == .projectRoot && !$0.isPointGuard }
            .map { workspace in
                SidebarNode(kind: .workspace(workspace), rowKey: rowKey("workspace", workspace.id, under: projectKey))
            }
        var children = rootWorkspaces
        for worktree in project.worktrees {
            let worktreeKey = rowKey("worktree", worktree.id, under: projectKey)
            let worktreeWorkspaces =
                workspaces
                .filter { $0.container == .worktree(worktree.id) }
                .map { workspace in
                    SidebarNode(
                        kind: .workspace(workspace), rowKey: rowKey("workspace", workspace.id, under: worktreeKey))
                }
            children.append(SidebarNode(kind: .worktree(worktree), rowKey: worktreeKey, children: worktreeWorkspaces))
        }
        return SidebarNode(kind: .project(project), rowKey: projectKey, children: children)
    }

    /// A machine row's key. Projects on it are keyed under this, so the same root on two
    /// machines is two different rows.
    static func machineKey(_ machineId: UUID) -> String {
        rowKey("machine", machineId.uuidString.lowercased())
    }

    /// The MACHINES section title.
    static func sectionHeaderNode(_ title: String) -> SidebarNode {
        SidebarNode(kind: .sectionHeader(title), rowKey: rowKey("section", title))
    }

    static func rowKey(_ kind: String, _ id: String, under parent: String? = nil) -> String {
        let component = "\(kind):\(Self.escape(id))"
        guard let parent else { return component }
        return "\(parent)/\(component)"
    }

    /// Reserved by the key grammar: the tier separator `:`, the parent separator `/`, and the
    /// escape itself. A project root is an arbitrary path, so these must not pass through raw
    /// — a root ending in `/worktree:wt-abc12345` would otherwise build the same key as that
    /// worktree row under a different project, and closing one would close both.
    private static func escape(_ id: String) -> String {
        id.replacingOccurrences(of: "%", with: "%25")
            .replacingOccurrences(of: ":", with: "%3A")
            .replacingOccurrences(of: "/", with: "%2F")
    }
}
