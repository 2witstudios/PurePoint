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

    init(kind: Kind, children: [SidebarNode] = []) {
        self.kind = kind
        self.children = children
    }

    var id: String {
        switch kind {
        case .project(let p): return p.projectId
        case .worktree(let w): return w.id
        case .workspace(let w): return w.id
        case .sectionHeader(let title): return "section:" + title
        case .machine(let m): return "machine-node:" + m.machine.id.uuidString.lowercased()
        }
    }
}
