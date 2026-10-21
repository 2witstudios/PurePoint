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
                await hub.loadAll(projectRoots: appState.projects.map(\.projectRoot))
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

        vc.onRemoveProject = { [viewCache] project in
            let projectRoot = project.projectRoot

            // A remote project is only hidden here: its files and agents stay on the machine.
            if project.isRemote {
                for agent in project.allAgents {
                    viewCache.remove(agentId: agent.id)
                }
                appState.closeProject(project.projectId)
                return
            }

            // Point Guard can still own this folder after it leaves the project sidebar.
            // Preserve both daemon files and attached terminal views in that case.
            guard appState.closeProject(projectRoot) else { return }

            for agent in project.allAgents {
                viewCache.remove(agentId: agent.id)
            }

            // Clean up .pu directory: delete manifest.json and agents/
            let puDir = URL(fileURLWithPath: projectRoot).appendingPathComponent(".pu")
            let manifestURL = puDir.appendingPathComponent("manifest.json")
            let agentsDirURL = puDir.appendingPathComponent("agents")

            try? FileManager.default.removeItem(at: manifestURL)
            try? FileManager.default.removeItem(at: agentsDirURL)
        }

        vc.onRemoveMachine = { [viewCache] machine in
            for project in machine.projects {
                for agent in project.allAgents { viewCache.remove(agentId: agent.id) }
            }
            appState.removeMachine(machine)
        }

    }
}
