import Foundation

// MARK: - CommandPaletteItem

enum CommandPaletteItem: Identifiable {
    case builtIn(AgentVariant)
    case agentDef(AgentDefinition)
    case swarm(SwarmDefinition)

    var id: String {
        switch self {
        case .builtIn(let v): "builtin:\(v.id):\(v.kind)"
        case .agentDef(let d): "agentdef:\(d.id)"
        case .swarm(let s): "swarm:\(s.id)"
        }
    }

    var displayName: String {
        switch self {
        case .builtIn(let v): v.displayName
        case .agentDef(let d): d.name
        case .swarm(let s): s.name
        }
    }

    var icon: String {
        switch self {
        case .builtIn(let v): v.icon
        case .agentDef(let d): d.icon ?? "cpu"
        case .swarm: "person.3"
        }
    }

    var subtitle: String {
        switch self {
        case .builtIn(let v): v.subtitle
        case .agentDef(let d):
            if let tmpl = d.template {
                "Template: \(tmpl)"
            } else if d.inlinePrompt != nil {
                "Inline prompt"
            } else {
                d.agentType
            }
        case .swarm(let s):
            "\(s.totalAgents) agent\(s.totalAgents == 1 ? "" : "s") across \(s.worktreeCount) worktree\(s.worktreeCount == 1 ? "" : "s")"
        }
    }

    var promptPlaceholder: String {
        switch self {
        case .builtIn(let v): v.promptPlaceholder
        case .agentDef(let d):
            if d.template != nil { "Override prompt (optional)..." } else { "Enter prompt..." }
        case .swarm: ""
        }
    }

    var categoryLabel: String? {
        switch self {
        case .builtIn: nil
        case .agentDef: "Agent"
        case .swarm: "Swarm"
        }
    }

    /// Text blob used for fuzzy-filtering in the palette.
    var searchableText: String {
        switch self {
        case .builtIn(let v):
            return "\(v.id) \(v.displayName) \(v.subtitle)"
        case .agentDef(let d):
            return "\(d.name) \(d.agentType) \(d.tags.joined(separator: " "))"
        case .swarm(let s):
            return s.name
        }
    }

    /// Whether selecting this item should skip the prompt phase and execute immediately.
    var skipsPromptPhase: Bool {
        switch self {
        case .builtIn: false
        case .agentDef(let d): d.inlinePrompt != nil
        case .swarm: true
        }
    }

    /// The worktree-style name field should be shown in the prompt phase.
    var showsNameField: Bool {
        switch self {
        case .builtIn(let v): v.kind == .worktree
        case .agentDef: false
        case .swarm: false
        }
    }

    static func buildItems(
        builtInVariants: [AgentVariant],
        agents: [AgentDefinition],
        swarms: [SwarmDefinition]
    ) -> [CommandPaletteItem] {
        let builtIns = builtInVariants.map { CommandPaletteItem.builtIn($0) }
        let agentItems =
            agents
            .filter(\.availableInCommandDialog)
            .map { CommandPaletteItem.agentDef($0) }
        let swarmItems = swarms.map { CommandPaletteItem.swarm($0) }
        return builtIns + agentItems + swarmItems
    }
}

// MARK: - CommandPaletteResult

enum CommandPaletteResult {
    case spawnBuiltIn(variant: AgentVariant, prompt: String?, name: String?)
    case spawnAgentDef(def: AgentDefinition, prompt: String?)
    case runSwarm(def: SwarmDefinition)
}
