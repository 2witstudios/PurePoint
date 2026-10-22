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

        #expect(titles(SidebarMenus.contextMenu(for: nodes[0])) == ["Kill All Agents (1)", "-", "Remove Project\u{2026}"])
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
        #expect(titles(SidebarMenus.contextMenu(for: nodes[0])) == ["Remove Project\u{2026}"])
        #expect(titles(SidebarMenus.contextMenu(for: nodes[0].children[0])) == ["Delete Worktree\u{2026}"])
    }

    @Test func givenAProjectDropShouldMoveItOnlyWithinItsOwnSection() {
        let a = makeProject(root: "/tmp/a", worktreeCount: 0)
        let b = makeProject(root: "/tmp/b", worktreeCount: 0)
        let c = makeProject(root: "/tmp/c", worktreeCount: 0)
        let machine = MachineState(machine: Machine(name: "m", sshHost: "m"), appState: nil, registry: nil)
        let roots = SidebarTree.build(projects: [a, b, c], machines: [machine], workspacesByProject: [:])
        let drop = { (id: String, parent: SidebarNode?, index: Int) in
            SidebarTree.projectDrop(projectId: id, parent: parent, childIndex: index, roots: roots)
                .map { "\($0.project.projectRoot)@\($0.index)" }
        }

        // Dropping before the first row moves to the top; past the last local row moves to the end.
        #expect(drop(c.projectId, nil, 0) == "/tmp/c@0")
        #expect(drop(a.projectId, nil, 3) == "/tmp/a@2")
        #expect(drop(a.projectId, nil, 2) == "/tmp/a@1")
        // Its own place, below the local projects, or under a machine it isn't on: no move.
        #expect(drop(a.projectId, nil, 0) == nil)
        #expect(drop(a.projectId, nil, 1) == nil)
        #expect(drop(a.projectId, nil, 4) == nil)
        #expect(drop(a.projectId, roots[4], 0) == nil)
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
        // A machine's project is only hidden; nothing on the machine changes.
        #expect(
            SidebarConfirmation.removeProject(project).message.hasSuffix(
                "Nothing on the remote machine is changed."))
    }
}
