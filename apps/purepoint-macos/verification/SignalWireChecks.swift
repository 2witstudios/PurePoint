import Foundation

@main enum SignalWireChecks {
    static func main() throws {
        let root = URL(fileURLWithPath: CommandLine.arguments[1])
        let fixture = try Data(contentsOf: root.appendingPathComponent("crates/pu-core/tests/fixtures/signals-status-v1.json"))
        let json = try JSONSerialization.jsonObject(with: fixture) as! [String: Any]
        let rootAgent = (json["agents"] as! [[String: Any]])[0]
        let wire = rootAgent["signal"] as! [String: Any]
        func snapshot(_ changes: [String: Any] = [:]) throws -> AgentSignalSnapshot {
            try JSONDecoder().decode(AgentSignalSnapshot.self, from: JSONSerialization.data(withJSONObject: wire.merging(changes) { _, new in new }))
        }
        let qualifiedTuple = try snapshot(["evidence": ["tool": "pi", "tool_version": "1.3.0", "adapter_version": "1", "confidence": "authoritative", "event_id": "e"]])
        precondition(qualifiedTuple.qualificationKey == "WyJwaSIsIjEuMy4wIiwiMSJd")
        let now = Date(timeIntervalSince1970: 1792195201)
        let current = try snapshot()
        precondition(current.presentation(agentID: "ag-root", now: now).label == "Permission")
        precondition(current.presentation(agentID: "other", now: now) == .unknown)
        precondition(current.presentation(agentID: "ag-root", sessionID: "other", now: now) == .unknown)
        precondition(current.presentation(agentID: "ag-root", available: false, now: now) == .unknown)
        precondition(current.presentation(agentID: "ag-root", now: Date(timeIntervalSince1970: 1792195199)) == .unknown)
        precondition(current.presentation(agentID: "ag-root", now: Date(timeIntervalSince1970: 1792195230)) == .unknown)
        for state in ["stale", "disconnected", "lost", "process_dead", "future_state"] {
            let changed = try snapshot(["observation": ["state": state, "last_received_at_ms": 1792195200000, "expires_at_ms": 1792195230000]])
            precondition(changed.presentation(agentID: "ag-root", now: now) == .unknown)
        }
        let future = try snapshot(["schema_version": 2])
        precondition(future.presentation(agentID: "ag-root", now: now) == .unknown)
        let noExpiry = try snapshot(["observation": ["state": "current", "last_received_at_ms": 1792195200000]])
        precondition(noExpiry.presentation(agentID: "ag-root", now: now) == .unknown)
        let working = try snapshot(["pending_attention": []])
        precondition(working.presentation(agentID: "ag-root", now: now).label == "Working")
        let idle = try snapshot(["activity": "idle", "pending_attention": [], "outcome": NSNull()])
        precondition(idle.presentation(agentID: "ag-root", now: now).label == "Idle",
            "A confirmed idle session with no completed turn must show Idle, not Unknown or Finished")
        precondition(!idle.presentation(agentID: "ag-root", now: now).needsAttention)
        let inferredIdle = try snapshot(["activity": "idle", "pending_attention": [], "outcome": NSNull(),
            "evidence": ["tool": "pi", "tool_version": "1.3.0", "adapter_version": "1", "confidence": "inferred", "event_id": "e"]])
        precondition(inferredIdle.presentation(agentID: "ag-root", now: now) == .unknown,
            "Idle is claimed only from authoritative evidence")
        let noEvidenceIdle = try snapshot(["activity": "idle", "pending_attention": [], "outcome": NSNull(), "evidence": NSNull()])
        precondition(noEvidenceIdle.presentation(agentID: "ag-root", now: now) == .unknown)
        let unknownActivity = try snapshot(["activity": "unknown", "pending_attention": [], "outcome": NSNull()])
        precondition(unknownActivity.presentation(agentID: "ag-root", now: now) == .unknown)
        var unboundIdentity = wire["identity"] as! [String: Any]
        unboundIdentity["native_session_id"] = ""
        let unboundIdle = try snapshot(["identity": unboundIdentity, "activity": "idle", "pending_attention": [], "outcome": NSNull()])
        precondition(unboundIdle.presentation(agentID: "ag-root", now: now).label == "Idle")
        precondition(unboundIdle.presentation(agentID: "ag-root", sessionID: "known-thread", now: now) == .unknown)
        for changes: [String: Any] in [["activity": "working", "pending_attention": [], "outcome": NSNull()], [:],
            ["activity": "idle", "pending_attention": [], "outcome": ["kind": "completed", "event_id": "outcome", "confidence": "authoritative"]]] {
            let unbound = try snapshot(changes.merging(["identity": unboundIdentity]) { _, new in new })
            precondition(unbound.presentation(agentID: "ag-root", now: now) == .unknown)
        }
        for (kind, label) in [("completed", "Finished"), ("errored", "Error"), ("interrupted", "Stopped"), ("future_outcome", "Unknown")] {
            let value = try snapshot(["activity": "idle", "pending_attention": [], "outcome": ["kind": kind, "event_id": "outcome", "confidence": "authoritative"]])
            precondition(value.presentation(agentID: "ag-root", now: now).label == label)
        }
        for confidence in ["heuristic", "source_confirmed", "future_confidence"] {
            let value = try snapshot(["activity": "idle", "pending_attention": [], "outcome": ["kind": "errored", "event_id": "outcome", "confidence": confidence]])
            precondition(value.presentation(agentID: "ag-root", now: now) == .unknown)
        }
        #if MOBILE_SIGNAL_CHECKS
        var agent: [String: Any] = ["id": "ag-root", "name": "Root", "agentType": "fixture", "state": "running", "screenAware": false, "signal": wire]
        func decodeAgent() throws -> ControlledAgent { try JSONDecoder().decode(ControlledAgent.self, from: JSONSerialization.data(withJSONObject: agent)) }
        let decoded = try decodeAgent()
        precondition(decoded.signal?.presentation(agentID: "ag-root", now: now).label == "Permission")
        agent.removeValue(forKey: "signal")
        let legacyAgent = try decodeAgent()
        precondition(legacyAgent.signal == nil)
        agent["signal"] = ["schema_version": "future malformed"]
        let malformedAgent = try decodeAgent()
        precondition(malformedAgent.signal == nil)
        agent["state"] = "suspended"
        let suspendedAgent = try decodeAgent()
        precondition(suspendedAgent.signalPresentation(now: now).label == "Suspended")
        precondition(suspendedAgent.signalPresentation(available: false, now: now) == .unknown)
        #else
        let response = try JSONDecoder().decode(DaemonResponse.self, from: fixture)
        guard case .statusReport(let worktrees, let agents) = response else { fatalError("Wrong status response") }
        precondition(agents[0].signal == current)
        let suspendedAgent = AgentModel(id: "suspended", name: "Suspended", agentType: "codex", status: .running,
            prompt: "", startedAt: "", suspended: true)
        precondition(suspendedAgent.signalPresentation(now: now).label == "Suspended")
        precondition(suspendedAgent.signalPresentation(available: false, now: now) == .unknown)
        var idleAgent = AgentModel(id: "ag-root", name: "Idle", agentType: "claude", status: .running,
            prompt: "", startedAt: "", signal: idle)
        precondition(idleAgent.signalPresentation(now: now).label == "Idle")
        idleAgent.signal = working
        precondition(idleAgent.signalPresentation(now: now).label == "Working")
        precondition(worktrees[0].agents["ag-worker"]?.signal?.presentation(agentID: "ag-worker", now: now).label == "Finished")
        var legacy = json
        var roots = [rootAgent]; roots[0].removeValue(forKey: "signal"); legacy["agents"] = roots
        let old = try JSONDecoder().decode(DaemonResponse.self, from: JSONSerialization.data(withJSONObject: legacy))
        guard case .statusReport(_, let oldAgents) = old else { fatalError("Wrong legacy response") }
        precondition(oldAgents[0].signal == nil)
        roots[0]["signal"] = ["schema_version": "future malformed"]; legacy["agents"] = roots
        _ = try JSONDecoder().decode(DaemonResponse.self, from: JSONSerialization.data(withJSONObject: legacy))
        #endif
        print("Pinned CORE wire checks passed: root/worktree, old daemon, malformed additive field, freshness, identity, authority and outcomes")
    }
}
