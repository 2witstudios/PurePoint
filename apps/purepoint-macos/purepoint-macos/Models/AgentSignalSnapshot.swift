import Foundation

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
        if activity == "idle" && outcome == nil {
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
