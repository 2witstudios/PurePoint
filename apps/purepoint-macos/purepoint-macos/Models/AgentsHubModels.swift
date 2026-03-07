import SwiftUI

struct SavedPrompt: Identifiable {
    let id: String  // same as name
    var name: String
    var description: String
    var agent: String
    var body: String
    var source: String
    var variables: [String]
    var command: String?

    init(from info: TemplateInfo) {
        self.id = "\(info.source):\(info.name)"
        self.name = info.name
        self.description = info.description
        self.agent = info.agent
        self.body = ""  // TemplateInfo doesn't include body (list response)
        self.source = info.source
        self.variables = info.variables
        self.command = info.command
    }

    init(
        name: String, description: String, agent: String, body: String, source: String,
        variables: [String], command: String? = nil
    ) {
        self.id = "\(source):\(name)"
        self.name = name
        self.description = description
        self.agent = agent
        self.body = body
        self.source = source
        self.variables = variables
        self.command = command
    }
}

struct AgentDefinition: Identifiable {
    let id: String  // same as name
    var name: String
    var agentType: String
    var template: String?
    var inlinePrompt: String?
    var tags: [String]
    var scope: String
    var availableInCommandDialog: Bool
    var icon: String?
    var command: String?

    init(from info: AgentDefInfo) {
        self.id = "\(info.scope):\(info.name)"
        self.name = info.name
        self.agentType = info.agentType
        self.template = info.template
        self.inlinePrompt = info.inlinePrompt
        self.tags = info.tags
        self.scope = info.scope
        self.availableInCommandDialog = info.availableInCommandDialog
        self.icon = info.icon
        self.command = info.command
    }

    init(
        name: String, agentType: String = "claude", template: String? = nil, inlinePrompt: String? = nil,
        tags: [String] = [], scope: String = "local", availableInCommandDialog: Bool = true, icon: String? = nil,
        command: String? = nil
    ) {
        self.id = "\(scope):\(name)"
        self.name = name
        self.agentType = agentType
        self.template = template
        self.inlinePrompt = inlinePrompt
        self.tags = tags
        self.scope = scope
        self.availableInCommandDialog = availableInCommandDialog
        self.icon = icon
        self.command = command
    }
}

struct SwarmDefinition: Identifiable {
    let id: String  // same as name
    var name: String
    var worktreeCount: Int
    var worktreeTemplate: String
    var roster: [SwarmRosterItem]
    var includeTerminal: Bool
    var scope: String

    var totalAgents: Int {
        worktreeCount * roster.reduce(0) { $0 + $1.quantity }
    }

    init(from info: SwarmDefInfo) {
        self.id = "\(info.scope):\(info.name)"
        self.name = info.name
        self.worktreeCount = info.worktreeCount
        self.worktreeTemplate = info.worktreeTemplate
        self.roster = info.roster.map { SwarmRosterItem(agentDef: $0.agentDef, role: $0.role, quantity: $0.quantity) }
        self.includeTerminal = info.includeTerminal
        self.scope = info.scope
    }

    init(
        name: String, worktreeCount: Int = 1, worktreeTemplate: String = "", roster: [SwarmRosterItem] = [],
        includeTerminal: Bool = false, scope: String = "local"
    ) {
        self.id = "\(scope):\(name)"
        self.name = name
        self.worktreeCount = worktreeCount
        self.worktreeTemplate = worktreeTemplate
        self.roster = roster
        self.includeTerminal = includeTerminal
        self.scope = scope
    }
}

struct SwarmRosterItem: Identifiable {
    let id = UUID()
    var agentDef: String
    var role: String
    var quantity: Int
}

enum PromptScopeChoice: String, CaseIterable, Identifiable {
    case global
    case project
    var id: String { rawValue }
    var title: String { rawValue.capitalized }
    var wireValue: String {
        switch self {
        case .global: "global"
        case .project: "local"
        }
    }
}

enum AgentTypes {
    static let all = ["claude", "codex", "opencode", "terminal"]
    static let withAny = [""] + all
}

enum AgentPromptSourceMode: String, CaseIterable, Identifiable {
    case library
    case inline
    var id: String { rawValue }
    var title: String {
        switch self {
        case .library: "From Prompts"
        case .inline: "Inline"
        }
    }
}
