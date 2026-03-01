import Foundation

struct MockAgent: Identifiable {
    let id: String
    let name: String
    let status: AgentStatus
}

struct MockTerminal: Identifiable {
    let id: String
    let name: String
}

struct MockWorktree: Identifiable {
    let id: String
    let branch: String
    let agents: [MockAgent]
    let terminals: [MockTerminal]
}

struct MockProject: Identifiable {
    let id: String
    let name: String
    let worktrees: [MockWorktree]
}

enum MockData {
    static let project = MockProject(
        id: "proj-1",
        name: "purepoint",
        worktrees: [
            MockWorktree(
                id: "wt-1",
                branch: "main",
                agents: [
                    MockAgent(id: "a-1", name: "Agent 1", status: .running),
                    MockAgent(id: "a-2", name: "Agent 2", status: .completed),
                ],
                terminals: [
                    MockTerminal(id: "t-1", name: "Terminal 1"),
                ]
            ),
            MockWorktree(
                id: "wt-2",
                branch: "pu/feature-auth",
                agents: [
                    MockAgent(id: "a-3", name: "Agent 3", status: .running),
                ],
                terminals: [
                    MockTerminal(id: "t-2", name: "Terminal 2"),
                ]
            ),
            MockWorktree(
                id: "wt-3",
                branch: "pu/fix-bug",
                agents: [
                    MockAgent(id: "a-4", name: "Agent 4", status: .failed),
                ],
                terminals: []
            ),
        ]
    )

    static let userProject = MockProject(
        id: "proj-user",
        name: "Jono",
        worktrees: [
            MockWorktree(
                id: "wt-user",
                branch: "Jono",
                agents: [
                    MockAgent(id: "a-user-1", name: "Orchestrator", status: .running),
                ],
                terminals: [
                    MockTerminal(id: "t-user-1", name: "Scratch"),
                ]
            ),
        ]
    )

    static let projects: [MockProject] = [userProject, project]
}
