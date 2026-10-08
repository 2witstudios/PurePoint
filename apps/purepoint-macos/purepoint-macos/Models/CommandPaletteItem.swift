import Foundation

// MARK: - CommandPaletteItem

/// A file that can be opened into a file pane.
struct PaletteFileEntry: Equatable, Sendable {
    let relativePath: String
    let absolutePath: String
}

enum CommandPaletteItem: Identifiable {
    case builtIn(AgentVariant)
    case agentDef(AgentDefinition)
    case swarm(SwarmDefinition)
    /// Opens the file navigator in a pane.
    case files
    /// Opens a specific file in a pane. Only offered once the user types a query.
    case file(PaletteFileEntry)

    var id: String {
        switch self {
        case .builtIn(let v): "builtin:\(v.id):\(v.kind)"
        case .agentDef(let d): "agentdef:\(d.id)"
        case .swarm(let s): "swarm:\(s.id)"
        case .files: "files"
        case .file(let f): "file:\(f.absolutePath)"
        }
    }

    var displayName: String {
        switch self {
        case .builtIn(let v): v.displayName
        case .agentDef(let d): d.name
        case .swarm(let s): s.name
        case .files: "Files"
        case .file(let f): (f.relativePath as NSString).lastPathComponent
        }
    }

    var icon: String {
        switch self {
        case .builtIn(let v): v.icon
        case .agentDef(let d): d.icon ?? "cpu"
        case .swarm: "person.3"
        case .files: "folder"
        case .file(let f): EditorLanguage.detect(from: (f.relativePath as NSString).lastPathComponent).icon
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
        case .files: "Browse and edit files"
        case .file(let f): f.relativePath
        }
    }

    var promptPlaceholder: String {
        switch self {
        case .builtIn(let v): v.promptPlaceholder
        case .agentDef(let d):
            if d.template != nil { "Override prompt (optional)..." } else { "Enter prompt..." }
        case .swarm, .files, .file: ""
        }
    }

    var categoryLabel: String? {
        switch self {
        case .builtIn, .files: nil
        case .agentDef: "Agent"
        case .swarm: "Swarm"
        case .file: "File"
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
        case .files:
            return "files file browser explorer navigator editor markdown"
        case .file(let f):
            return f.relativePath
        }
    }

    /// Whether selecting this item should skip the prompt phase and execute immediately.
    var skipsPromptPhase: Bool {
        switch self {
        case .builtIn: false
        case .agentDef(let d): d.inlinePrompt != nil
        case .swarm, .files, .file: true
        }
    }

    /// Only shown when the user has typed a query (one per file would flood the default list).
    var isQueryOnly: Bool {
        if case .file = self { return true }
        return false
    }

    /// The worktree-style name field should be shown in the prompt phase.
    var showsNameField: Bool {
        switch self {
        case .builtIn(let v): v.kind == .worktree
        case .agentDef, .swarm, .files, .file: false
        }
    }

    /// Whether the prompt text field should be shown in the prompt phase.
    /// Worktree items only need a name, not a prompt.
    var showsPromptField: Bool {
        switch self {
        case .builtIn(let v): v.kind != .worktree
        case .agentDef: true
        case .swarm, .files, .file: false
        }
    }

    static func buildItems(
        builtInVariants: [AgentVariant],
        agents: [AgentDefinition],
        swarms: [SwarmDefinition],
        includeFiles: Bool = false,
        files: [PaletteFileEntry] = [],
        preferredOrder: [String] = []
    ) -> [CommandPaletteItem] {
        let builtIns = builtInVariants.map { CommandPaletteItem.builtIn($0) }
        let agentItems =
            agents
            .filter(\.availableInCommandDialog)
            .map { CommandPaletteItem.agentDef($0) }
        let swarmItems = swarms.map { CommandPaletteItem.swarm($0) }
        let fileItems: [CommandPaletteItem] = includeFiles ? [.files] + files.map { .file($0) } : []
        let items = builtIns + fileItems + agentItems + swarmItems
        var ranks: [String: Int] = [:]
        for (index, id) in preferredOrder.enumerated() where ranks[id] == nil {
            ranks[id] = index
        }
        return items.enumerated().sorted { lhs, rhs in
            let left = ranks[lhs.element.id] ?? Int.max
            let right = ranks[rhs.element.id] ?? Int.max
            return left == right ? lhs.offset < rhs.offset : left < right
        }.map(\.element)
    }
}

// MARK: - CommandPaletteResult

enum CommandPaletteResult {
    case spawnBuiltIn(variant: AgentVariant, prompt: String?, name: String?)
    case spawnAgentDef(def: AgentDefinition, prompt: String?)
    case runSwarm(def: SwarmDefinition)
    case createWorktree(name: String?)
    /// Show the file navigator in the pane, optionally opened on `path`.
    case openFilePane(path: String?)
}
