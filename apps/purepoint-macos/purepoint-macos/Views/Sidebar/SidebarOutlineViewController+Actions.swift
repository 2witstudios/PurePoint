import Foundation

// What the sidebar's menu actions do, shared by the Mac controller and its iPad twin. Each
// platform supplies `showConfirmation(_:action:)` and `beginRename(_:)`; everything else here
// is the same on both.

@MainActor
extension SidebarOutlineViewController {
    /// Runs a menu action chosen on a row.
    func perform(_ action: SidebarMenuAction, on node: SidebarNode) {
        switch (action, node.kind) {
        case (.connectMachine, .machine(let machine)):
            machine.connect()
        case (.disconnectMachine, .machine(let machine)):
            confirmDisconnect(machine)
        case (.startDaemon, .machine(let machine)):
            machine.startDaemon()
        case (.showHiddenProjects, .machine(let machine)):
            machine.showHiddenProjects()
        case (.dismissMachineError, .machine(let machine)):
            machine.actionError = nil
        case (.removeMachine, .machine(let machine)):
            confirmRemove(machine)
        case (.renameWorkspace, .workspace):
            beginRename(node)
        case (.killWorkspace, .workspace(let workspace)):
            killWorkspace(workspace)
        case (.killWorktreeAgents, .worktree(let worktree)):
            guard let project = findProject(forWorktreeId: worktree.id) else { return }
            showConfirmation(.killWorktreeAgents(worktree)) {
                self.onKillWorktreeAgents?(project, worktree.id)
            }
        case (.deleteWorktree, .worktree(let worktree)):
            guard let project = findProject(forWorktreeId: worktree.id) else { return }
            showConfirmation(.deleteWorktree(worktree)) {
                self.onDeleteWorktree?(project, worktree.id)
            }
        case (.killAllProjectAgents, .project(let project)):
            showConfirmation(.killAllProjectAgents(project)) {
                self.onKillAllProjectAgents?(project)
            }
        case (.retryLayout, .project(let project)):
            project.retryRemoteLayout()
        case (.hideProject, .project(let project)):
            showConfirmation(.hideProject(project)) {
                self.onHideProject?(project)
            }
        case (.removeProject, .project(let project)):
            showConfirmation(.removeProject(project)) {
                self.onRemoveProject?(project)
            }
        case (.newAgentInWorktree, .worktree(let worktree)):
            guard let project = findProject(forWorktreeId: worktree.id) else { return }
            onShowCommandPalette?(project, .worktree(worktree.id), false)
        case (.newTerminalInWorktree, .worktree(let worktree)):
            guard let project = findProject(forWorktreeId: worktree.id),
                let model = project.worktrees.first(where: { $0.id == worktree.id })
            else { return }
            onAddTerminal?(project, model)
        default:
            break
        }
    }

    /// Return on a machine row connects or disconnects it.
    func toggleConnection(of machine: MachineState) {
        if machine.isConnectedOrConnecting { confirmDisconnect(machine) } else { machine.connect() }
    }

    func confirmDisconnect(_ machine: MachineState) {
        let unsaved = machine.unsavedFileNames
        guard !unsaved.isEmpty else {
            machine.disconnect(remember: true)
            return
        }
        showConfirmation(.disconnect(machine, unsaved: unsaved)) {
            machine.disconnect(remember: true)
        }
    }

    func confirmRemove(_ machine: MachineState) {
        showConfirmation(.removeMachine(machine)) {
            self.onRemoveMachine?(machine)
        }
    }

    private func killWorkspace(_ workspace: Workspace) {
        guard let project = findProject(forWorkspaceId: workspace.id) else { return }

        let agentCount = workspace.agentIds.count
        guard agentCount > 1 else {
            onKillWorkspace?(project, workspace.id)
            return
        }
        showConfirmation(.closeWorkspace(agentCount: agentCount)) {
            self.onKillWorkspace?(project, workspace.id)
        }
    }

    // MARK: - Project Lookup

    /// Local projects and every machine's projects, in sidebar order.
    var allProjects: [ProjectState] { SidebarTree.allProjects(in: projectNodes) }

    func findProject(byRoot projectId: String) -> ProjectState? {
        allProjects.first { $0.projectId == projectId }
    }

    func findProject(forWorktreeId wtId: String) -> ProjectState? {
        allProjects.first { $0.worktrees.contains(where: { $0.id == wtId }) }
    }

    func findProject(forWorkspaceId workspaceId: String) -> ProjectState? {
        allProjects.first { workspacesByProject[$0.projectId]?.contains(where: { $0.id == workspaceId }) == true }
    }

    func findWorkspace(id workspaceId: String) -> Workspace? {
        for (_, list) in workspacesByProject {
            if let match = list.first(where: { $0.id == workspaceId }) { return match }
        }
        return nil
    }
}
