import Foundation
import Testing

@testable import PurePoint

// The sidebar's shared model: the tree, expansion and selection both the Mac outline and its
// iPad twin draw. Runs in both test targets.

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
struct SidebarOutlineModelTests {

    // MARK: - Tree

    @Test func givenMachinesShouldAppendAMachinesSectionWithEachMachinesProjectsUnderIt() {
        let machine = MachineState(machine: Machine(name: "studio", sshHost: "me@studio"), appState: nil, registry: nil)
        let nodes = SidebarTree.build(projects: [], machines: [machine], workspacesByProject: [:])

        #expect(nodes.map(\.id) == ["section:MACHINES", "machine-node:" + machine.id.uuidString.lowercased()])
        #expect(nodes[1].rowKey == SidebarNode.machineKey(machine.id))
    }

    @Test func givenNoMachinesShouldShowNoMachinesSection() {
        let project = makeProject(root: "/tmp/p", worktreeCount: 1)
        let nodes = SidebarTree.build(projects: [project], machines: [], workspacesByProject: [:])
        #expect(nodes.map(\.id) == [project.projectId])
        #expect(nodes[0].children.map(\.id) == ["wt-0"])
    }

    @Test func givenAClosedRowShouldNotOpenItsChildrenButShouldKeepTheirClosedState() {
        let project = makeProject(root: "/tmp/expansion", worktreeCount: 2)
        let nodes = SidebarTree.build(projects: [project], machines: [], workspacesByProject: [:])
        let projectKey = nodes[0].rowKey
        let closedChild = nodes[0].children[0].rowKey

        let closedParent = SidebarTree.expansion(nodes, closedRowKeys: [projectKey, closedChild])
        #expect(closedParent.open.isEmpty)
        #expect(closedParent.closed == [projectKey, closedChild])

        let openParent = SidebarTree.expansion(nodes, closedRowKeys: [closedChild, "gone"])
        #expect(openParent.open.map(\.rowKey) == [projectKey, nodes[0].children[1].rowKey])
        #expect(openParent.closed == [closedChild])
    }

    @Test func givenAProjectThatHasNotLoadedShouldNotCountAsSettled() {
        let project = makeProject(root: "/tmp/settled", worktreeCount: 0)
        let nodes = { SidebarTree.build(projects: [project], machines: [], workspacesByProject: [:]) }
        #expect(!SidebarTree.isSettled(nodes()))
        project.markWorktreesLoadedForTesting()
        #expect(SidebarTree.isSettled(nodes()))
    }

    @Test func givenARowShouldReportTheSelectionTheMacReports() {
        let project = makeProject(root: "/tmp/select", worktreeCount: 1)
        let workspace = Workspace.adopting(agentId: "agent-0", container: .worktree("wt-0"))
        let machine = MachineState(machine: Machine(name: "m", sshHost: "m"), appState: nil, registry: nil)
        let nodes = SidebarTree.build(
            projects: [project], machines: [machine], workspacesByProject: [project.projectId: [workspace]])
        let worktree = nodes[0].children[0]

        #expect(SidebarTree.selection(for: nodes[0]) == .project(project.projectId))
        #expect(SidebarTree.selection(for: worktree) == .worktree("wt-0"))
        #expect(SidebarTree.selection(for: worktree.children[0]) == .workspace(workspace.id))
        #expect(SidebarTree.selection(for: nodes[1]) == nil)
        #expect(SidebarTree.selection(for: nodes[2]) == nil)
        #expect(!SidebarTree.isSelectable(nodes[1]))
        #expect(SidebarTree.isSelectable(nodes[2]))
        #expect(SidebarTree.targetId(for: .nav(.agents)) == nil)
        #expect(SidebarTree.targetId(for: .channel(project.projectId)) == project.projectId)
        #expect(SidebarTree.ancestors(where: { $0.id == workspace.id }, in: nodes).map(\.id) == [project.projectId, "wt-0"])
        #expect(SidebarTree.worktreeNode(id: "wt-0", in: nodes) === worktree)
    }
}
