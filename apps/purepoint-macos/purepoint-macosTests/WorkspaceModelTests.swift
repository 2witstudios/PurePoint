import XCTest

@testable import PurePoint

final class WorkspaceModelTests: XCTestCase {
    private func agent(name: String = "Original", prompt: String = "", sessionId: String? = nil) -> AgentModel {
        AgentModel(
            id: "ag-1", name: name, agentType: "claude", status: .running,
            prompt: prompt, startedAt: "", sessionId: sessionId)
    }

    private func worktree(name: String = "Original", path: String = "/tmp/wt-1", agent: AgentModel) -> WorktreeModel {
        WorktreeModel(
            id: "wt-1", name: name, path: path, branch: "pu/test", status: "active",
            agents: [agent])
    }

    func testGivenRootAgentRenameShouldDetectChangedSnapshotWithoutStatusChange() {
        // ProjectState.mergeRootAgents uses array equality to decide whether to apply a snapshot.
        XCTAssertNotEqual([agent()], [agent(name: "Renamed")])
    }

    func testGivenWorktreeAgentRenameShouldDetectChangedSnapshotWithoutStatusChange() {
        // Nested agent names must also reach ProjectState.mergeWorktrees.
        XCTAssertNotEqual(
            [worktree(agent: agent())],
            [worktree(agent: agent(name: "Renamed"))])
    }

    func testGivenUpdatedMetadataShouldDetectChangedSnapshot() {
        XCTAssertNotEqual(agent(), agent(prompt: "Updated prompt"))
        XCTAssertNotEqual(agent(), agent(sessionId: "new-session"))
        XCTAssertNotEqual(worktree(agent: agent()), worktree(name: "Renamed", agent: agent()))
        XCTAssertNotEqual(worktree(agent: agent()), worktree(path: "/tmp/moved", agent: agent()))
    }

    func testGivenUnchangedSnapshotShouldAvoidRedundantUpdate() {
        XCTAssertEqual([agent()], [agent()])
        XCTAssertEqual([worktree(agent: agent())], [worktree(agent: agent())])
    }
}
