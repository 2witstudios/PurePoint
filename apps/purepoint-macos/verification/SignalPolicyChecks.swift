import Foundation

@main enum SignalPolicyChecks {
    static func main() {
        let permission = AgentSignalPresentation(label: "Permission", eventID: "request", needsAttention: true)
        var policy = AgentSignalAlertPolicy()
        let agent = AgentSignalAlertPolicy.Agent(identity: "host/project/agent", launch: "one", presentation: permission)
        precondition(policy.project([agent, agent], qualifiedAttention: { _ in false }).badge == 1)
        precondition(policy.project([agent], qualifiedAttention: { _ in false }).alerts.isEmpty)
        var freshPolicy = AgentSignalAlertPolicy()
        precondition(freshPolicy.project([agent], qualifiedAttention: { _ in true }).alerts.count == 1)
        precondition(freshPolicy.project([agent], qualifiedAttention: { _ in true }).alerts.isEmpty)
        let unknown = AgentSignalAlertPolicy.Agent(identity: agent.identity, launch: "one", presentation: .unknown)
        precondition(freshPolicy.project([unknown], qualifiedAttention: { _ in true }).badge == 0)
        let next = AgentSignalAlertPolicy.Agent(identity: agent.identity, launch: "two", presentation: permission)
        precondition(freshPolicy.project([next], qualifiedAttention: { _ in true }).alerts.count == 1)
        let error = AgentSignalAlertPolicy.Agent(identity: agent.identity, launch: "two", presentation: .init(label: "Error", eventID: "failure", needsAttention: true))
        var errors = AgentSignalAlertPolicy()
        precondition(errors.project([error]).badge == 1)
        precondition(errors.project([error], acknowledgedErrors: [error.eventKey!]).badge == 0)
        precondition(error.presentation.label == "Error")
        var restored = AgentSignalAlertPolicy(deliveredKeys: freshPolicy.deliveredKeys)
        precondition(restored.project([next], qualifiedAttention: { _ in true }).alerts.isEmpty)
        print("Signal policy: distinct agents, gated attention, launch deduplication and stale withdrawal passed")
    }
}
