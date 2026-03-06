import SwiftUI

struct SavedPrompt: Identifiable {
    let id: String  // same as name
    var name: String
    var description: String
    var agent: String
    var body: String
    var source: String
    var variables: [String]

    init(from info: TemplateInfo) {
        self.id = "\(info.source):\(info.name)"
        self.name = info.name
        self.description = info.description
        self.agent = info.agent
        self.body = ""  // TemplateInfo doesn't include body (list response)
        self.source = info.source
        self.variables = info.variables
    }

    init(name: String, description: String, agent: String, body: String, source: String, variables: [String]) {
        self.id = name
        self.name = name
        self.description = description
        self.agent = agent
        self.body = body
        self.source = source
        self.variables = variables
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
    }

    init(name: String, agentType: String = "claude", template: String? = nil, inlinePrompt: String? = nil, tags: [String] = [], scope: String = "local", availableInCommandDialog: Bool = true, icon: String? = nil) {
        self.id = name
        self.name = name
        self.agentType = agentType
        self.template = template
        self.inlinePrompt = inlinePrompt
        self.tags = tags
        self.scope = scope
        self.availableInCommandDialog = availableInCommandDialog
        self.icon = icon
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

    init(name: String, worktreeCount: Int = 1, worktreeTemplate: String = "", roster: [SwarmRosterItem] = [], includeTerminal: Bool = false, scope: String = "local") {
        self.id = name
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
