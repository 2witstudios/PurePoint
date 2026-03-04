import SwiftUI

/// NSViewControllerRepresentable wrapping SidebarOutlineViewController.
/// Bridges SwiftUI state (selection, appState, gridState) to the AppKit outline view.
struct SidebarOutlineView: NSViewControllerRepresentable {
    @Binding var selection: SidebarSelection?
    var appState: AppState
    var gridState: GridState

    func makeNSViewController(context: Context) -> SidebarOutlineViewController {
        let vc = SidebarOutlineViewController()

        vc.onSelectionChanged = { newSelection in
            selection = newSelection
        }

        vc.onShowCommandPalette = { project, sel, includeWorktree in
            let variants = includeWorktree ? AgentVariant.variantsWithWorktree : AgentVariant.allVariants
            CommandPalettePanel.show(relativeTo: NSApp.keyWindow, variants: variants) { variant, prompt, name in
                project.createAgent(variant: variant, prompt: prompt, name: name, selection: sel)
            }
        }

        vc.onAddTerminal = { project, worktree in
            project.createAgent(variant: .terminal, prompt: nil, selection: .worktree(worktree.id))
        }

        vc.onKillAgent = { project, agentId in
            project.killAgent(agentId)
        }

        vc.onKillWorktreeAgents = { project, worktreeId in
            project.killWorktreeAgents(worktreeId)
        }

        vc.onRenameAgent = { project, agentId, newName in
            project.renameAgent(agentId, to: newName)
        }

        vc.onDeleteWorktree = { project, worktreeId in
            project.deleteWorktree(worktreeId)
        }

        vc.onKillAllProjectAgents = { project in
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
