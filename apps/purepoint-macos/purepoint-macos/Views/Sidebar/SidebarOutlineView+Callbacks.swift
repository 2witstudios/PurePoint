import SwiftUI

// The sidebar controller's callbacks, shared by the Mac `NSViewControllerRepresentable` and its
// iPad `UIViewControllerRepresentable` twin, so a row action does the same thing on both.

extension SidebarOutlineView {
    @MainActor
    static func wireCallbacks(
        _ vc: SidebarOutlineViewController, selection: Binding<SidebarSelection?>, appState: AppState,
        registry: WorkspaceRegistry, viewCache: TerminalViewCache, settingsState: SettingsState
    ) {
        vc.onSelectionChanged = { newSelection in
            selection.wrappedValue = newSelection
        }

        let hub = appState.agentsHubState
        vc.onShowCommandPalette = { project, sel, includeWorktree in
            let builtIns = includeWorktree ? AgentVariant.variantsWithWorktree : AgentVariant.allVariants
            let window = NSApp.keyWindow
            CommandPalettePanel.requestOpen {
                await hub.loadAll(projectRoots: appState.libraryProjectRoots)
                let items = CommandPaletteItem.buildItems(
                    builtInVariants: builtIns,
                    agents: hub.agents,
                    swarms: includeWorktree ? hub.swarms : [],
                    preferredOrder: settingsState.commandPaletteOrder
                )

                CommandPalettePanel.show(
                    relativeTo: window, items: items, usePointGuardNames: settingsState.usePointGuardNames
                ) { result in
                    project.handlePaletteResult(result, selection: sel, hub: hub)
                }
            }
        }

        vc.onAddTerminal = { project, worktree in
            if settingsState.usePointGuardNames {
                project.createAgent(agent: "terminal", prompt: "", selection: .worktree(worktree.id))
            } else {
                CommandPalettePanel.requestOpen {
                    CommandPalettePanel.show(
                        relativeTo: NSApp.keyWindow, items: [.builtIn(.terminal)],
                        usePointGuardNames: false, initialItem: .builtIn(.terminal)
                    ) { result in
                        project.handlePaletteResult(result, selection: .worktree(worktree.id), hub: hub)
                    }
                }
            }
        }

        // Closing the row closes everything in it — agent tabs (killed via the registry's
        // onCloseAgent) and file tabs alike — so a workspace of only files can be closed too.
        vc.onKillWorkspace = { [registry] _, workspaceId in
            registry.closeWorkspace(workspaceId)
        }

        vc.onKillWorktreeAgents = { [viewCache] project, worktreeId in
            for agent in project.allAgents where project.worktreeId(forAgentId: agent.id) == worktreeId {
                viewCache.remove(agentId: agent.id)
            }
            project.killWorktreeAgents(worktreeId)
        }

        vc.onRenameAgent = { project, agentId, newName in
            project.renameAgent(agentId, to: newName)
        }

        vc.onDeleteWorktree = { [viewCache] project, worktreeId in
            for agent in project.allAgents where project.worktreeId(forAgentId: agent.id) == worktreeId {
                viewCache.remove(agentId: agent.id)
            }
            project.deleteWorktree(worktreeId)
        }

        vc.onKillAllProjectAgents = { [viewCache] project in
            for agent in project.allAgents {
                viewCache.remove(agentId: agent.id)
            }
            project.killAllAgents()
        }

        let dropTerminalViews = { [viewCache] (project: ProjectState) in
            for agent in project.allAgents {
                viewCache.remove(agentId: agent.id)
            }
        }

        // Hiding is this device's choice: the project and its agents stay on the machine.
        vc.onHideProject = { project in
            dropTerminalViews(project)
            appState.closeProject(project.projectId)
        }

        // The machine's daemon forgets the project, so it leaves every device's sidebar. Its
        // files, including .pu/, stay: adding the folder again brings the project back.
        vc.onRemoveProject = { project in
            Task { @MainActor in
                do {
                    try await appState.removeProject(project)
                } catch {
                    let message = "Could not remove \(project.projectName): \(error.localizedDescription)"
                    if project.isRemote, let machine = appState.machine(for: project) {
                        machine.actionError = message
                    } else {
                        appState.daemonError = message
                    }
                    return
                }
                // Point Guard may still own a local folder; its terminal views stay attached.
                guard appState.projectState(forRoot: project.projectId) == nil else { return }
                dropTerminalViews(project)
            }
        }

        vc.onRemoveMachine = { [viewCache] machine in
            for project in machine.projects {
                for agent in project.allAgents { viewCache.remove(agentId: agent.id) }
            }
            appState.removeMachine(machine)
        }

    }
}
