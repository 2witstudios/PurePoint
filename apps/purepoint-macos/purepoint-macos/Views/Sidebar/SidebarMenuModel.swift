import Foundation

// The sidebar's menus and the confirmations their actions ask for, shared by the Mac outline
// and its iPad twin. Each platform only turns these entries into its own menu.

// MARK: - Menus

/// Everything a sidebar row's menus can do.
enum SidebarMenuAction: Equatable {
    case connectMachine
    case disconnectMachine
    case startDaemon
    case showHiddenProjects
    case dismissMachineError
    case removeMachine
    case renameWorkspace
    case killWorkspace
    case killWorktreeAgents
    case deleteWorktree
    case killAllProjectAgents
    case retryLayout
    case removeProject
    case moveProjectUp
    case moveProjectDown
    case newAgentInWorktree
    case newTerminalInWorktree
}

/// One line of a sidebar menu.
enum SidebarMenuEntry: Equatable {
    case action(String, SidebarMenuAction, isEnabled: Bool = true)
    case separator
}

@MainActor
enum SidebarMenus {
    /// A row's context menu, top to bottom. Empty for rows without one.
    static func contextMenu(for node: SidebarNode) -> [SidebarMenuEntry] {
        switch node.kind {
        case .workspace(let workspace): return workspaceMenu(workspace)
        case .worktree(let worktree): return worktreeMenu(worktree)
        case .project(let project): return projectMenu(project)
        case .machine(let machine): return machineMenu(machine)
        case .sectionHeader: return []
        }
    }

    /// The menu behind a worktree row's add button.
    static let worktreeAddMenu: [SidebarMenuEntry] = [
        .action("New Agent", .newAgentInWorktree),
        .action("New Terminal", .newTerminalInWorktree),
    ]

    /// The machine's menu actions, reachable from VoiceOver's actions rotor.
    static func machineAccessibilityActions(_ machine: MachineState) -> [(name: String, action: SidebarMenuAction)] {
        var actions: [(name: String, action: SidebarMenuAction)] = []
        if machine.isConnectedOrConnecting {
            actions.append(("Disconnect", .disconnectMachine))
        } else {
            actions.append(("Connect", .connectMachine))
        }
        if machine.state == .connected(.stopped), !machine.isStartingDaemon {
            actions.append(("Start Daemon", .startDaemon))
        }
        if machine.hiddenProjectCount > 0 {
            actions.append(("Show Hidden Projects", .showHiddenProjects))
        }
        if machine.actionError != nil {
            actions.append(("Dismiss Error", .dismissMachineError))
        }
        actions.append(("Remove Machine", .removeMachine))
        return actions
    }

    private static func machineMenu(_ machine: MachineState) -> [SidebarMenuEntry] {
        var entries: [SidebarMenuEntry] = []
        if machine.isConnectedOrConnecting {
            entries.append(.action("Disconnect", .disconnectMachine))
        } else {
            entries.append(.action("Connect", .connectMachine))
        }
        if machine.state == .connected(.stopped) {
            let title = machine.isStartingDaemon ? "Starting Daemon\u{2026}" : "Start Daemon"
            entries.append(.action(title, .startDaemon, isEnabled: !machine.isStartingDaemon))
        }
        if machine.hiddenProjectCount > 0 {
            entries.append(.action("Show Hidden Projects (\(machine.hiddenProjectCount))", .showHiddenProjects))
        }
        if machine.actionError != nil {
            entries.append(.action("Dismiss Error", .dismissMachineError))
        }
        entries.append(.separator)
        entries.append(.action("Remove Machine\u{2026}", .removeMachine))
        return entries
    }

    private static func workspaceMenu(_ workspace: Workspace) -> [SidebarMenuEntry] {
        let agentCount = workspace.agentIds.count
        let title =
            switch agentCount {
            case 0: "Close Workspace"
            case 1 where workspace.tabCount == 1: "Kill Agent"
            case 1: "Close Workspace (1 agent)"
            default: "Close Workspace (\(agentCount) agents)"
            }
        return [.action("Rename\u{2026}", .renameWorkspace), .separator, .action(title, .killWorkspace)]
    }

    private static func worktreeMenu(_ worktree: WorktreeModel) -> [SidebarMenuEntry] {
        var entries: [SidebarMenuEntry] = []
        let aliveCount = worktree.agents.filter { $0.status.isAlive }.count
        if aliveCount > 0 {
            entries.append(.action("Kill All Agents (\(aliveCount))", .killWorktreeAgents))
            entries.append(.separator)
        }
        entries.append(.action("Delete Worktree\u{2026}", .deleteWorktree))
        return entries
    }

    private static func projectMenu(_ project: ProjectState) -> [SidebarMenuEntry] {
        var entries: [SidebarMenuEntry] = []
        let aliveCount = project.allAgents.filter { $0.status.isAlive }.count
        if aliveCount > 0 {
            entries.append(.action("Kill All Agents (\(aliveCount))", .killAllProjectAgents))
            entries.append(.separator)
        }
        if project.isRemote, project.registry?.layoutErrorsByProject[project.projectId] != nil {
            entries.append(.action("Retry Layout", .retryLayout))
        }
        if let siblings = project.appState?.siblingProjects(of: project), siblings.count > 1,
            let index = siblings.firstIndex(where: { $0 === project })
        {
            entries.append(.action("Move Up", .moveProjectUp, isEnabled: index > 0))
            entries.append(.action("Move Down", .moveProjectDown, isEnabled: index < siblings.count - 1))
            entries.append(.separator)
        }
        entries.append(.action("Remove Project\u{2026}", .removeProject))
        return entries
    }
}

// MARK: - Confirmations

/// A confirmation a sidebar action asks for before it runs.
struct SidebarConfirmation: Equatable {
    let title: String
    let message: String
    var confirmTitle = "Kill All"
}

@MainActor
extension SidebarConfirmation {
    static func disconnect(_ machine: MachineState, unsaved: [String]) -> SidebarConfirmation {
        SidebarConfirmation(
            title: "Disconnect \(machine.machine.name)?",
            message: "Its projects close in PurePoint until you connect again." + unsavedWarning(unsaved),
            confirmTitle: "Disconnect")
    }

    static func removeMachine(_ machine: MachineState) -> SidebarConfirmation {
        SidebarConfirmation(
            title: "Remove \(machine.machine.name)?",
            message:
                "PurePoint will close its connection and forget it. The daemon, projects and agents on "
                + "\(machine.machine.name) keep running and are not changed."
                + unsavedWarning(machine.unsavedFileNames),
            confirmTitle: "Remove")
    }

    static func closeWorkspace(agentCount: Int) -> SidebarConfirmation {
        SidebarConfirmation(
            title: "Close this workspace?",
            message: "This will kill \(agentCount) agents across its panes.",
            confirmTitle: "Close")
    }

    static func killWorktreeAgents(_ worktree: WorktreeModel) -> SidebarConfirmation {
        let aliveCount = worktree.agents.filter { $0.status.isAlive }.count
        return SidebarConfirmation(
            title: "Kill All Agents in \(worktree.branch)?",
            message: "This will kill \(aliveCount) running agent\(aliveCount == 1 ? "" : "s") in this worktree.")
    }

    static func deleteWorktree(_ worktree: WorktreeModel) -> SidebarConfirmation {
        let aliveCount = worktree.agents.filter { $0.status.isAlive }.count
        let agentNote =
            aliveCount > 0
            ? "This will kill \(aliveCount) running agent\(aliveCount == 1 ? "" : "s"), "
            : "This will "
        return SidebarConfirmation(
            title: "Delete worktree \(worktree.branch)?",
            message: "\(agentNote)remove the worktree directory, and delete the branch locally and from GitHub.",
            confirmTitle: "Delete")
    }

    static func killAllProjectAgents(_ project: ProjectState) -> SidebarConfirmation {
        let aliveCount = project.allAgents.filter { $0.status.isAlive }.count
        return SidebarConfirmation(
            title: "Kill All Agents in \(project.projectName)?",
            message: "This will kill \(aliveCount) running agent\(aliveCount == 1 ? "" : "s") in this project.")
    }

    static func removeProject(_ project: ProjectState) -> SidebarConfirmation {
        let unsaved =
            project.isRemote
            ? project.appState?.machines.first { $0.machine.id == project.key.machineId }?.unsavedFileNames(in: project)
                ?? []
            : []
        return SidebarConfirmation(
            title: "Remove \(project.projectName)?",
            message: project.isRemote
                ? "This hides the project in PurePoint on \(deviceName). Nothing on the remote machine is changed."
                    + unsavedWarning(unsaved)
                : "This will close the project in PurePoint and delete .pu/manifest.json and .pu/agents/. "
                    + "The project folder will not be deleted.",
            confirmTitle: "Remove")
    }

    /// The device a hidden project stays hidden on.
    private static var deviceName: String {
        #if os(macOS)
        "this Mac"
        #else
        "this iPad"
        #endif
    }

    /// Closing a remote project drops its file tabs, edits included.
    private static func unsavedWarning(_ names: [String]) -> String {
        names.isEmpty ? "" : " Unsaved changes to \(names.joined(separator: ", ")) will be lost."
    }
}
