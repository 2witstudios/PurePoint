import Foundation

struct AgentVariant: Identifiable {
    let id: String
    let displayName: String
    let icon: String              // SF Symbol
    let subtitle: String
    let promptPlaceholder: String
    let kind: Kind

    enum Kind { case agent, terminal }

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

    static let allVariants: [AgentVariant] = [claude, codex, opencode, terminal]
}
