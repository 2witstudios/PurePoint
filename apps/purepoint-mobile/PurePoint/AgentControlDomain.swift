import Foundation

enum AgentTerminalPermission: String, Decodable, Sendable {
    case off, view, control
}

struct AgentInventory: Decodable, Sendable {
    let machines: [AgentMachine]
}

struct AgentMachine: Decodable, Identifiable, Sendable {
    let id: String
    let name: String
    let online: Bool
    let protocolVersion: Int?
    let screenMayBeInexact: Bool
    let error: String?
    let complete: Bool
    let agents: [ControlledAgent]

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

struct ControlledAgent: Decodable, Identifiable, Sendable {
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

    mutating func begin() -> AgentSubmission? {
        guard pending == nil, !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty, text.utf8.count <= 65536
        else { return nil }
        let submission = AgentSubmission(id: UUID(), text: text)
        pending = submission; outcome = nil; reason = nil
        return submission
    }
    mutating func finish(_ submission: AgentSubmission, outcome: AgentPromptOutcome, reason: String?) {
        guard pending?.id == submission.id else { return }
        pending = nil; self.outcome = outcome; self.reason = reason
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
