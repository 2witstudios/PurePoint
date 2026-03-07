import SwiftUI

/// NSViewControllerRepresentable wrapping SidebarOutlineViewController.
/// Bridges SwiftUI state (selection, appState, gridState) to the AppKit outline view.
struct SidebarOutlineView: NSViewControllerRepresentable {
    @Binding var selection: SidebarSelection?
    var appState: AppState
    var gridState: GridState
    var viewCache: TerminalViewCache
    var onOutlineViewReady: ((NSOutlineView) -> Void)?

    func makeNSViewController(context: Context) -> SidebarOutlineViewController {
        let vc = SidebarOutlineViewController()
        onOutlineViewReady?(vc.outlineView)

        vc.onSelectionChanged = { newSelection in
            selection = newSelection
        }

        let hub = appState.agentsHubState
        vc.onShowCommandPalette = { project, sel, includeWorktree in
            let builtIns = includeWorktree ? AgentVariant.variantsWithWorktree : AgentVariant.allVariants
            let items = CommandPaletteItem.buildItems(
                builtInVariants: builtIns,
                agents: hub.agents,
                swarms: includeWorktree ? hub.swarms : []
            )
            Task { await hub.loadAll(projectRoot: project.projectRoot) }

            CommandPalettePanel.show(relativeTo: NSApp.keyWindow, items: items) { result in
                project.handlePaletteResult(result, selection: sel, hub: hub)
            }
        }

        vc.onAddTerminal = { project, worktree in
            project.createAgent(agent: "terminal", prompt: "", selection: .worktree(worktree.id))
        }

        vc.onKillAgent = { [viewCache] project, agentId in
            viewCache.remove(agentId: agentId)
            project.killAgent(agentId)
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

        return vc
    }

    func updateNSViewController(_ vc: SidebarOutlineViewController, context: Context) {
        // Update grid state
        vc.gridOwnerAgentId = gridState.ownerAgentId
        vc.hiddenAgentIds = gridState.childAgentIds
        vc.gridProjectRoot = gridState.projectRoot

        // Rebuild tree when data changes
        vc.rebuildNodes(projects: appState.projects)

        // Handle incoming programmatic selection (e.g., pendingSelectAgentId)
        vc.selectNode(for: selection)
    }
}
