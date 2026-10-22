import Foundation
import Testing

@testable import PurePoint

// The sidebar's shared menus and confirmations, which both the Mac outline and its iPad twin
// offer. Runs in both test targets.

private struct ModelTestWorkspaceService: WorkspaceService {
    func loadWorkspace(projectRoot: String) async throws -> WorkspaceSnapshot {
        WorkspaceSnapshot(worktrees: [], rootAgents: [])
    }

    func manifestPath(projectRoot: String) -> String { "/dev/null" }
}

@MainActor
private func makeAgent(_ id: String, status: AgentStatus = .running) -> AgentModel {
    AgentModel(id: id, name: id, agentType: "claude", status: status, prompt: "", startedAt: "")
}

/// The machine every test project lives on, so a root names the same project each time.
private let testMachineId = UUID()

@MainActor
private func makeProject(root: String, worktreeCount: Int, status: AgentStatus = .running) -> ProjectState {
    let project = ProjectState(
        projectRoot: root, service: ModelTestWorkspaceService(), registry: nil, endpoint: .machine(testMachineId),
        host: UnavailableProjectHost())
    project.worktrees = (0..<worktreeCount).map { index in
        WorktreeModel(
            id: "wt-\(index)", name: "wt-\(index)", path: "/tmp/wt-\(index)", branch: "branch-\(index)",
            status: "active", agents: [makeAgent("agent-\(index)", status: status)])
    }
    return project
}

/// A machine's project offers hiding on this device and removing from the machine.
#if os(macOS)
private let hideTitle = "Hide on This Mac\u{2026}"
#else
private let hideTitle = "Hide on This iPad\u{2026}"
#endif
private let remoteProjectActions = [hideTitle, "Remove from Machine\u{2026}"]

@Suite(.serialized)
@MainActor
struct SidebarMenuModelTests {

    private func titles(_ entries: [SidebarMenuEntry]) -> [String] {
        entries.map {
            switch $0 {
            case .action(let title, _, _): title
            case .separator: "-"
            }
        }
    }

    // MARK: - Menus

    @Test func givenEachRowTypeShouldOfferTheMacContextMenu() {
        let project = makeProject(root: "/tmp/menus", worktreeCount: 1)
        let single = Workspace.adopting(agentId: "agent-0", container: .worktree("wt-0"))
        let machine = MachineState(machine: Machine(name: "m", sshHost: "m"), appState: nil, registry: nil)
        let nodes = SidebarTree.build(
            projects: [project], machines: [machine], workspacesByProject: [project.projectId: [single]])

        #expect(titles(SidebarMenus.contextMenu(for: nodes[0])) == ["Kill All Agents (1)", "-"] + remoteProjectActions)
        #expect(
            titles(SidebarMenus.contextMenu(for: nodes[0].children[0]))
                == ["Kill All Agents (1)", "-", "Delete Worktree\u{2026}"])
        #expect(titles(SidebarMenus.contextMenu(for: nodes[0].children[0].children[0])) == ["Rename\u{2026}", "-", "Kill Agent"])
        #expect(SidebarMenus.contextMenu(for: nodes[1]).isEmpty)
        #expect(titles(SidebarMenus.contextMenu(for: nodes[2])) == ["Connect", "-", "Remove Machine\u{2026}"])

        machine.actionError = "ssh failed"
        #expect(titles(SidebarMenus.contextMenu(for: nodes[2])) == ["Connect", "Dismiss Error", "-", "Remove Machine\u{2026}"])
        #expect(
            SidebarMenus.machineAccessibilityActions(machine).map(\.name) == ["Connect", "Dismiss Error", "Remove Machine"])
    }

    @Test func givenDeadAgentsShouldOfferOnlyTheDestructiveRowActions() {
        let project = makeProject(root: "/tmp/dead", worktreeCount: 1, status: .broken)
        let nodes = SidebarTree.build(projects: [project], machines: [], workspacesByProject: [:])
        #expect(titles(SidebarMenus.contextMenu(for: nodes[0])) == remoteProjectActions)
        #expect(titles(SidebarMenus.contextMenu(for: nodes[0].children[0])) == ["Delete Worktree\u{2026}"])
    }

    @Test func givenAWorktreeAddButtonShouldOfferANewAgentOrTerminal() {
        #expect(titles(SidebarMenus.worktreeAddMenu) == ["New Agent", "New Terminal"])
    }

    // MARK: - Confirmations

    @Test func givenDestructiveActionsShouldAskWithTheMacWording() {
        let project = makeProject(root: "/tmp/confirm", worktreeCount: 1)
        let worktree = project.worktrees[0]

        #expect(
            SidebarConfirmation.deleteWorktree(worktree)
                == SidebarConfirmation(
                    title: "Delete worktree branch-0?",
                    message:
                        "This will kill 1 running agent, remove the worktree directory, and delete the branch locally and from GitHub.",
                    confirmTitle: "Delete"))
        #expect(SidebarConfirmation.killWorktreeAgents(worktree).confirmTitle == "Kill All")
        #expect(SidebarConfirmation.closeWorkspace(agentCount: 3).message == "This will kill 3 agents across its panes.")
        // Hiding changes nothing on the machine.
        #expect(
            SidebarConfirmation.hideProject(project).message.hasSuffix(
                "Nothing on the remote machine is changed."))
        // Removing does, for every device, but keeps the files and suspends running agents.
        #expect(
            SidebarConfirmation.removeProject(project)
                == SidebarConfirmation(
                    title: "Remove confirm from Machine?",
                    message: "It disappears from PurePoint on Machine and every device connected to it."
                        + " Its 1 running agent is suspended and can be resumed once the project is added again."
                        + " The folder, including .pu/, is not changed.",
                    confirmTitle: "Remove"))
    }
}
