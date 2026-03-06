import Foundation

struct AgentVariant: Identifiable {
    let id: String
    let displayName: String
    let icon: String              // SF Symbol
    let subtitle: String
    let promptPlaceholder: String
    let kind: Kind

    enum Kind { case agent, terminal, worktree }

    // MARK: - Built-in Variants

    static let claude = AgentVariant(
        id: "claude",
        displayName: "Claude",
        icon: "circle.fill",
        subtitle: "AI coding agent",
        promptPlaceholder: "Enter prompt...",
        kind: .agent
    )

    static let codex = AgentVariant(
        id: "codex",
        displayName: "Codex",
        icon: "diamond",
        subtitle: "OpenAI coding CLI",
        promptPlaceholder: "Enter prompt...",
        kind: .agent
    )

    static let opencode = AgentVariant(
        id: "opencode",
        displayName: "OpenCode",
        icon: "diamond",
        subtitle: "Open-source agent",
        promptPlaceholder: "Enter prompt...",
        kind: .agent
    )

    static let terminal = AgentVariant(
        id: "terminal",
        displayName: "Terminal",
        icon: "terminal",
        subtitle: "Shell session",
        promptPlaceholder: "Enter initial command (optional)...",
        kind: .terminal
    )

    // id matches claude variant — it's passed to the daemon as the agent type to spawn
    static let worktree = AgentVariant(
        id: "claude",
        displayName: "Worktree",
        icon: "arrow.triangle.branch",
        subtitle: "Isolated branch with agent",
        promptPlaceholder: "Enter prompt...",
        kind: .worktree
    )

    static let allVariants: [AgentVariant] = [claude, codex, opencode, terminal]
    static let variantsWithWorktree: [AgentVariant] = allVariants + [worktree]
}
