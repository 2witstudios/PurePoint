import Foundation

/// A projection of current producer facts, independent of process liveness.
nonisolated struct AgentSignalPresentation: Equatable, Sendable {
    let label: String
    let eventID: String?
    let needsAttention: Bool
    var detail = ""
    var accessibilityDescription: String { detail.isEmpty ? label : "\(label), \(detail)" }
    static let unknown = Self(label: "Unknown", eventID: nil, needsAttention: false)
}

nonisolated struct AgentSignalAlertPolicy {
    nonisolated struct Agent: Sendable {
        let identity: String
        let launch: String
        let presentation: AgentSignalPresentation
        var qualificationKey: String? = nil
        var macOSActionable = false
        var destination: AgentSignalDestination? = nil
        var eventKey: String? {
            presentation.eventID.map { event in
                // Components are opaque and may contain separators.
                (try? JSONEncoder().encode([identity, launch, String(destination?.generation ?? 0), destination?.nativeSessionID ?? "", event]).base64EncodedString()) ?? ""
            }
        }
    }
    private var delivered = Set<String>()
    private var order: [String] = []

    init(deliveredKeys: [String] = []) {
        var seen = Set<String>()
        order = Array(deliveredKeys.suffix(4096)).filter { seen.insert($0).inserted }
        delivered = Set(order)
    }
    var deliveredKeys: [String] { order }

    /// Qualification is supplied only after the destination's exact-request route is proven.
    mutating func project(_ agents: [Agent], qualifiedAttention: (Agent) -> Bool = { _ in false }, acknowledgedErrors: Set<String> = []) -> (badge: Int, alerts: [Agent]) {
        let badge = Set(agents.filter { $0.presentation.needsAttention && !($0.presentation.label == "Error" && $0.eventKey.map(acknowledgedErrors.contains) == true) }.map(\.identity)).count
        var alerts: [Agent] = []
        for agent in agents {
            guard let key = agent.eventKey, !delivered.contains(key), !acknowledgedErrors.contains(key) else { continue }
            if delivered.count >= 4096 {
                let current = Set(agents.compactMap(\.eventKey))
                if let old = order.first(where: { !current.contains($0) }) {
                    delivered.remove(old); order.removeAll { $0 == old }
                } else { continue }
            }
            // Keep state/badges current even when notification delivery is unqualified.
            guard !agent.presentation.needsAttention || qualifiedAttention(agent) else { continue }
            guard agent.presentation.needsAttention || agent.presentation.label == "Finished" else { continue }
            delivered.insert(key)
            order.append(key)
            alerts.append(agent)
        }
        return (badge, alerts)
    }
}
