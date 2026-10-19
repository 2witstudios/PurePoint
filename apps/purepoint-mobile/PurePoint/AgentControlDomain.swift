import Foundation

enum AgentTerminalPermission: String, Decodable, Sendable {
    case off, view, control
}

struct AgentInventory: Codable, Sendable {
    let machines: [AgentMachine]
}

struct AgentMachine: Codable, Identifiable, Sendable {
    let id: String
    let name: String
    let online: Bool
    let protocolVersion: Int?
    let screenMayBeInexact: Bool
    let error: String?
    let complete: Bool
    let agents: [ControlledAgent]

    var registeredProjects: [WorkspaceProject]?
    var worktrees: [WorkspaceWorktree]?
    var metadataStatus: String?
    var metadataError: String?

    enum CodingKeys: String, CodingKey {
        case id, name, online, protocolVersion, screenMayBeInexact, error, complete, agents, worktrees, metadataStatus, metadataError
        case registeredProjects = "projects"
    }

    var projects: [AgentProject] {
        Dictionary(grouping: agents, by: { $0.projectRoot ?? "" }).map { root, agents in
            AgentProject(
                id: root,
                agents: agents.sorted {
                    if ($0.phase == "dialog") != ($1.phase == "dialog") { return $0.phase == "dialog" }
                    return $0.name == $1.name
                        ? $0.id < $1.id : $0.name.localizedStandardCompare($1.name) == .orderedAscending
                })
        }.sorted {
            let left = $0.agents.contains { $0.phase == "dialog" }
            let right = $1.agents.contains { $0.phase == "dialog" }
            return left != right ? left : $0.id < $1.id
        }
    }
}

struct AgentProject: Identifiable, Sendable {
    let id: String
    let agents: [ControlledAgent]
    var name: String { id.isEmpty ? "Other agents" : URL(fileURLWithPath: id).lastPathComponent }
}

struct ControlledAgent: Codable, Identifiable, Sendable {
    let id: String
    let name: String
    let agentType: String
    let state: String
    let projectRoot: String?
    let worktreeId: String?
    let screenAware: Bool
    let phase: String?
    let idleSeconds: Double?
    let startedAt: String?
    var signal: AgentSignalSnapshot? = nil

    func signalPresentation(available: Bool = true, now: Date = Date()) -> AgentSignalPresentation {
        guard available else { return .unknown }
        if state == "suspended" { return .init(label: "Suspended", eventID: nil, needsAttention: false) }
        guard let signal, signal.identity.worktree_id == worktreeId else { return .unknown }
        return signal.presentation(agentID: id, available: state == "running", now: now)
    }

    var promptBlockReason: String? {
        switch phase {
        case "dialog": return "Agent is waiting on a prompt. Use the keys below."
        case "has_draft":
            return "Someone is already typing in this agent. Finish or clear that draft on its terminal before sending."
        case "busy": return "Agent is busy. Wait until it is ready before sending a prompt."
        default:
            if state == "broken" { return "This agent has stopped." }
            if screenAware && phase != "input_ready" {
                return "Agent readiness is unknown. Refresh before sending a prompt."
            }
            return nil
        }
    }
    var idleLabel: String {
        guard let idleSeconds, idleSeconds.isFinite, idleSeconds >= 0 else { return "Idle time unavailable" }
        let seconds = Int(min(idleSeconds, 315_360_000))
        return seconds < 60 ? "Idle \(seconds)s" : seconds < 3600 ? "Idle \(seconds / 60)m" : "Idle \(seconds / 3600)h"
    }
}

struct AgentScope: Hashable, Sendable {
    let host: String
    let machine: String
    let agent: String
    var storageKey: String {
        // Encoding components avoids collisions with separators in opaque identities.
        (try? JSONEncoder().encode([host, machine, agent]).base64EncodedString()) ?? ""
    }
}

enum AgentPromptOutcome: String, Decodable, Sendable {
    case confirmed, sent, blocked, failed
    var label: String {
        switch self {
        case .confirmed: return "Confirmed"
        case .sent: return "Sent · unconfirmed"
        case .blocked: return "Blocked"
        case .failed: return "Failed"
        }
    }
}

struct AgentPromptResult: Decodable, Sendable {
    let outcome: AgentPromptOutcome
    let reason: String?
}

struct AgentSubmission: Equatable, Sendable {
    let id: UUID
    let text: String
}

struct AgentComposer: Sendable {
    var text = ""
    private(set) var pending: AgentSubmission?
    private(set) var outcome: AgentPromptOutcome?
    private(set) var reason: String?
    private(set) var deliveryUncertain = false

    mutating func begin() -> AgentSubmission? {
        guard pending == nil, !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty, text.utf8.count <= 65536
        else { return nil }
        let submission = AgentSubmission(id: UUID(), text: text)
        pending = submission; outcome = nil; reason = nil; deliveryUncertain = false
        return submission
    }
    mutating func finish(_ submission: AgentSubmission, outcome: AgentPromptOutcome, reason: String?, uncertain: Bool = false) {
        guard pending?.id == submission.id else { return }
        pending = nil; self.outcome = outcome; self.reason = reason; deliveryUncertain = uncertain
        if (outcome == .confirmed || outcome == .sent) && text == submission.text { text = "" }
    }
}

enum AgentQuickKey: String, CaseIterable, Identifiable, Sendable {
    case escape = "Esc", interrupt = "Ctrl-C", tab = "Tab", enter = "Enter"
    case up = "ArrowUp", down = "ArrowDown", left = "ArrowLeft", right = "ArrowRight", yes = "y", no = "n"
    var id: String { rawValue }
    var label: String {
        switch self {
        case .up: return "↑";
        case .down: return "↓";
        case .left: return "←";
        case .right: return "→";
        default: return rawValue
        }
    }
    // Specification of the bridge's fixed mapping, never sent as arbitrary IPC.
    var bytes: [UInt8] {
        switch self {
        case .escape: return [27]
        case .interrupt: return [3]
        case .tab: return [9]
        case .enter: return [13]
        case .up: return [27, 91, 65]
        case .down: return [27, 91, 66]
        case .left: return [27, 91, 68]
        case .right: return [27, 91, 67]
        case .yes: return [121]
        case .no: return [110]
        }
    }
}

struct AgentTerminalOpen: Decodable, Sendable {
    let streamId: String
    let machine: String
    let agentId: String
    let screenMayBeInexact: Bool
}

struct AgentTerminalEvent: Decodable, Sendable {
    let type: String
    let streamId: String
    var data: String?
    var cols: Int?
    var rows: Int?
    var reason: String?
    var byteCount: Int { (data?.utf8.count ?? 0) + (reason?.utf8.count ?? 0) + streamId.utf8.count + 128 }
}

// Only alive while one open RPC is outstanding. Discarded on every screen,
// host, connection or permission change; never reused by a later open.
struct AgentPendingOpen: Sendable {
    private(set) var events: [AgentTerminalEvent] = []
    private(set) var bytes = 0
    private(set) var overflowed = false
    mutating func append(_ event: AgentTerminalEvent) {
        guard !overflowed else { return }
        guard events.count < 1024, bytes + event.byteCount <= 512 * 1024 else {
            events = []; bytes = 0; overflowed = true; return
        }
        events.append(event); bytes += event.byteCount
    }
}

struct AgentControlFailure: LocalizedError, Sendable {
    let message: String
    var errorDescription: String? { message }
}

enum AgentTerminalUpdate: Sendable {
    case output(String)
    case size(Int, Int)
    var text: String? { if case .output(let text) = self { return text }; return nil }
}

struct AgentPromptRecovery: Codable, Identifiable, Equatable, Sendable {
    let id: UUID
    let text: String
    var status: String
    var reason: String?
}

struct AgentNavigationPreferences: Codable, Equatable, Sendable {
    private(set) var pins: [String] = []
    private(set) var interactions: [String: Double] = [:]
    private(set) var hidden: Set<String> = []
    func isPinned(_ scope: AgentScope) -> Bool { pins.contains(scope.storageKey) }
    func isHidden(_ scope: AgentScope) -> Bool { hidden.contains(scope.storageKey) }
    mutating func pin(_ scope: AgentScope) {
        hidden.remove(scope.storageKey)
        if !isPinned(scope) { pins.append(scope.storageKey) }
    }
    mutating func unpin(_ scope: AgentScope) { pins.removeAll { $0 == scope.storageKey } }
    mutating func hide(_ scope: AgentScope) { unpin(scope); hidden.insert(scope.storageKey) }
    mutating func interact(_ scope: AgentScope, at time: Double = Date().timeIntervalSince1970) {
        hidden.remove(scope.storageKey); interactions[scope.storageKey] = time
    }
    func recency(_ scope: AgentScope, startedAt: String?) -> Double {
        interactions[scope.storageKey] ?? startedAt.flatMap { ISO8601DateFormatter().date(from: $0)?.timeIntervalSince1970 } ?? 0
    }
}

struct AgentShortcut: Identifiable {
    let scope: AgentScope
    let machine: AgentMachine
    let agent: ControlledAgent
    var id: String { scope.storageKey }
}

struct WorkspaceProject: Codable, Identifiable, Sendable {
    let id: String
    let name: String
    let available: Bool
    let error: String?
}
struct WorkspaceWorktree: Codable, Identifiable, Sendable {
    let projectId: String
    let id: String
    let name: String
    let path: String
    let branch: String
    let status: String
    let agentIds: [String]
}
struct WorkspaceCatalog: Decodable, Sendable {
    struct Agent: Decodable, Identifiable, Sendable { let id: String; let name: String; let available: Bool }
    struct Preset: Decodable, Identifiable, Sendable { let id: String; let name: String; let agentType: String; let prompt: String }
    let agents: [Agent]
    let presets: [Preset]
    let terminalAvailable: Bool
    let complete: Bool
}
struct WorkspaceOperation: Codable, Sendable {
    struct Detail: Codable, Sendable { let agentId: String?; let worktreeId: String? }
    let operationId: String
    let state: String
    let message: String?
    let detail: Detail?
}

struct WorkspaceFormDraft: Codable, Equatable {
    var prompt = ""
    var name = ""
    var base = ""
    var agentType = ""
    var preset = ""
    var destination = "root"
    var worktree = ""
    var planMode = false
    var operationId: String?
    var state: String?
    var message: String?
    mutating func beginOperation() -> String? {
        guard operationId == nil else { return nil }
        let id = UUID().uuidString
        operationId = id; state = "pending"; message = nil
        return id
    }
    mutating func markUncertain(_ reason: String) { state = "uncertain"; message = reason }
}

struct WorkspaceLocation: Equatable, Sendable {
    let host: String
    let machine: String
    let project: String
    let worktree: String?
    var storageKey: String { Self.key([host, machine, project, worktree ?? ""]) }
    static func key(_ components: [String]) -> String { (try? JSONEncoder().encode(components).base64EncodedString()) ?? "" }
    static func locate(_ scope: AgentScope, machines: [AgentMachine]) -> WorkspaceLocation? {
        guard let machine = machines.first(where: { $0.id == scope.machine }),
              let agent = machine.agents.first(where: { $0.id == scope.agent }),
              let project = machine.registeredProjects?.first(where: { $0.id == agent.projectRoot }) else { return nil }
        if let worktree = agent.worktreeId,
           machine.worktrees?.contains(where: { $0.id == worktree && $0.projectId == project.id }) != true { return nil }
        return WorkspaceLocation(host: scope.host, machine: scope.machine, project: project.id, worktree: agent.worktreeId)
    }
}

struct WorkspaceNavigationMemory: Equatable, Sendable {
    var projectAnchors: [String: String] = [:]
    var workspaceAnchors: [String: String] = [:]
    var collapsed: Set<String> = []
    mutating func locate(_ location: WorkspaceLocation, agent: String) {
        collapsed.remove(location.storageKey)
        let workspace = WorkspaceLocation(host: location.host, machine: location.machine, project: location.project, worktree: nil)
        workspaceAnchors[workspace.storageKey] = agent
    }
}

struct MobileOverlayState: Equatable, Sendable {
    var navigator = false
    var history = false
    private(set) var dialogSelection: Int?
    mutating func openNavigator() { history = false; navigator = true }
    mutating func openHistory() { navigator = false; history = true }
    mutating func dialogArrived(selection: Int) { navigator = false; history = false; dialogSelection = selection }
    func shouldPresentDialog(selection: Int) -> Bool { dialogSelection == selection && !navigator && !history }
}

extension AgentInventory {
    // Treat repeated registry rows as one scoped identity. Keeping the first
    // projection prevents later duplicates from changing associations.
    var normalized: AgentInventory {
        var seenMachines = Set<String>()
        return AgentInventory(machines: machines.compactMap { machine in
            guard seenMachines.insert(machine.id).inserted else { return nil }
            var seenAgents = Set<String>(), seenProjects = Set<String>(), seenWorktrees = Set<String>()
            return AgentMachine(
                id: machine.id, name: machine.name, online: machine.online,
                protocolVersion: machine.protocolVersion, screenMayBeInexact: machine.screenMayBeInexact,
                error: machine.error, complete: machine.complete,
                agents: machine.agents.filter { seenAgents.insert($0.id).inserted },
                registeredProjects: machine.registeredProjects?.filter { seenProjects.insert($0.id).inserted },
                worktrees: machine.worktrees?.filter { seenWorktrees.insert(WorkspaceLocation.key([$0.projectId, $0.id])).inserted },
                metadataStatus: machine.metadataStatus, metadataError: machine.metadataError)
        })
    }
}

struct AgentDestinationContext: Equatable, Sendable {
    let agentName: String
    let machineName: String
    let projectLabel: String
    let worktreeLabel: String
    let status: String

    static func resolve(scope: AgentScope, machines: [AgentMachine], connected: Bool) -> AgentDestinationContext {
        let machine = machines.first { $0.id == scope.machine }
        let agent = machine?.agents.first { $0.id == scope.agent }
        let project = machine?.registeredProjects?.first { $0.id == agent?.projectRoot }
        let worktree = machine?.worktrees?.first { $0.projectId == agent?.projectRoot && $0.id == agent?.worktreeId }
        let available = connected && machine?.online == true && machine?.metadataStatus != "error" && agent != nil
        let state = available
            ? [agent?.state, agent?.phase == "dialog" ? "Answer needed" : agent?.phase?.replacingOccurrences(of: "_", with: " ")].compactMap { $0 }.joined(separator: " · ")
            : (agent == nil ? "Unavailable" : "Unavailable · cached context")
        return AgentDestinationContext(
            agentName: agent.flatMap { $0.name.isEmpty ? nil : $0.name } ?? scope.agent,
            machineName: machine.flatMap { $0.name.isEmpty ? nil : $0.name } ?? scope.machine,
            projectLabel: project.map { $0.name.isEmpty ? $0.id : $0.name }
                ?? agent?.projectRoot.map { $0 + " · project metadata unavailable" } ?? "Project unavailable",
            worktreeLabel: worktree.map { $0.name + " · " + $0.branch }
                ?? agent?.worktreeId.map { $0 + " · worktree metadata unavailable" }
                ?? (project != nil && agent != nil ? "Project root" : "Location unavailable"),
            status: state)
    }
}





/// A projection of current producer facts, independent of process liveness.
nonisolated struct AgentSignalPresentation: Equatable, Sendable {
    let label: String
    let eventID: String?
    let needsAttention: Bool
    var detail = ""
    var accessibilityDescription: String { detail.isEmpty ? label : "\(label), \(detail)" }
    static let unknown = Self(label: "Unknown", eventID: nil, needsAttention: false)
}


/// Additive v1 wire facts. Strings deliberately preserve unknown future enum values.
nonisolated struct AgentSignalSnapshot: Codable, Equatable, Sendable {
    nonisolated struct Identity: Codable, Equatable, Sendable {
        let agent_id: String
        let worktree_id: String?
        let launch_id: String
        let generation: UInt64
        let native_session_id: String
    }
    nonisolated struct Actionability: Codable, Equatable, Sendable {
        let macos: Bool?
        let ios: Bool?
    }
    nonisolated struct Attention: Codable, Equatable, Sendable {
        let request_id: String
        let kind: String
        let event_id: String
        let turn_id: String?
        let confidence: String
        let actionability: Actionability?
    }
    nonisolated struct Outcome: Codable, Equatable, Sendable {
        let kind: String
        let event_id: String
        let turn_id: String?
        let confidence: String
    }
    nonisolated struct Observation: Codable, Equatable, Sendable {
        let state: String
        let last_received_at_ms: Int64?
        let expires_at_ms: Int64?
    }
    nonisolated struct Evidence: Codable, Equatable, Sendable {
        let tool: String
        let tool_version: String
        let adapter_version: String
        let confidence: String
        let event_id: String
    }
    let schema_version: UInt32
    let identity: Identity
    let activity: String
    let pending_attention: [Attention]
    let outcome: Outcome?
    let observation: Observation
    let evidence: Evidence?

    var qualificationKey: String? {
        guard let evidence else { return nil }
        return (try? JSONEncoder().encode([evidence.tool, evidence.tool_version, evidence.adapter_version]).base64EncodedString())
    }

    func presentation(agentID: String, sessionID: String? = nil, available: Bool = true, now: Date = Date()) -> AgentSignalPresentation {
        let milliseconds = now.timeIntervalSince1970 * 1000
        let launchIdle = identity.native_session_id.isEmpty && sessionID == nil
            && activity == "idle" && pending_attention.isEmpty && outcome == nil
            && evidence?.confidence == "authoritative"
        guard available, schema_version == 1, identity.agent_id == agentID,
            !identity.launch_id.isEmpty, !identity.native_session_id.isEmpty || launchIdle,
            sessionID == nil || sessionID == identity.native_session_id,
            observation.state == "current", let received = observation.last_received_at_ms,
            let expires = observation.expires_at_ms,
            milliseconds >= Double(received), milliseconds < Double(expires)
        else { return .unknown }
        let pending = pending_attention.filter { $0.confidence == "authoritative" && !$0.request_id.isEmpty && !$0.event_id.isEmpty }
        if let request = pending.first(where: { $0.kind == "permission" }) ?? pending.first(where: { $0.kind == "question" }) {
            return .init(label: request.kind == "permission" ? "Permission" : "Question", eventID: request.event_id, needsAttention: true, detail: "\(pending.count) pending requests: " + pending.map { "\($0.kind) \($0.request_id)" }.joined(separator: ", "))
        }
        if activity == "working" { return .init(label: "Working", eventID: nil, needsAttention: false) }
        if activity == "idle" && outcome == nil && evidence?.confidence == "authoritative" {
            return .init(label: "Idle", eventID: nil, needsAttention: false)
        }
        guard activity == "idle" || activity == "unknown", let outcome,
            outcome.confidence == "authoritative", !outcome.event_id.isEmpty else { return .unknown }
        switch outcome.kind {
        case "completed": return .init(label: "Finished", eventID: outcome.event_id, needsAttention: false)
        case "errored": return .init(label: "Error", eventID: outcome.event_id, needsAttention: true)
        case "interrupted": return .init(label: "Stopped", eventID: outcome.event_id, needsAttention: false)
        default: return .unknown
        }
    }
}

extension ControlledAgent {
    enum CodingKeys: String, CodingKey {
        case id, name, agentType, state, projectRoot, worktreeId, screenAware, phase, idleSeconds, startedAt, signal
    }
    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        id = try c.decode(String.self, forKey: .id)
        name = try c.decode(String.self, forKey: .name)
        agentType = try c.decode(String.self, forKey: .agentType)
        state = try c.decode(String.self, forKey: .state)
        projectRoot = try c.decodeIfPresent(String.self, forKey: .projectRoot)
        worktreeId = try c.decodeIfPresent(String.self, forKey: .worktreeId)
        screenAware = try c.decode(Bool.self, forKey: .screenAware)
        phase = try c.decodeIfPresent(String.self, forKey: .phase)
        idleSeconds = try c.decodeIfPresent(Double.self, forKey: .idleSeconds)
        startedAt = try c.decodeIfPresent(String.self, forKey: .startedAt)
        signal = try? c.decodeIfPresent(AgentSignalSnapshot.self, forKey: .signal)
    }
}
