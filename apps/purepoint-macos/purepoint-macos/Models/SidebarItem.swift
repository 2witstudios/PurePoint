import SwiftUI

enum SidebarNavItem: String, CaseIterable, Identifiable {
    case dashboard
    case agents
    case schedule
    case memory

    var id: String { rawValue }

    var title: String {
        rawValue.capitalized
    }

    var icon: String {
        switch self {
        case .dashboard: "square.grid.2x2"
        case .agents:    "cpu"
        case .schedule:  "calendar"
        case .memory:    "brain"
        }
    }
}

enum SidebarSelection: Hashable {
    case nav(SidebarNavItem)
    case agent(String)
    case terminal(String)
    case worktree(String)
    case project(String)    // projectRoot path
}
