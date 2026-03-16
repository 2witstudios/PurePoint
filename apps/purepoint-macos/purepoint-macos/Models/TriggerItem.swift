import SwiftUI

// MARK: - Trigger Event

enum TriggerEvent: String, CaseIterable, Identifiable {
    case agentIdle = "agent_idle"
    case preCommit = "pre_commit"
    case prePush = "pre_push"

    var id: String { rawValue }

    var label: String {
        switch self {
        case .agentIdle: "Agent Idle"
        case .preCommit: "Pre-Commit"
        case .prePush: "Pre-Push"
        }
    }

    var icon: String {
        switch self {
        case .agentIdle: "moon.zzz"
        case .preCommit: "checkmark.circle"
        case .prePush: "arrow.up.circle"
        }
    }

    var color: Color {
        switch self {
        case .agentIdle: .blue
        case .preCommit: .orange
        case .prePush: .purple
        }
    }
}

// MARK: - Trigger Item

struct TriggerItem: Identifiable {
    let id: String
    let name: String
    let description: String?
    let event: TriggerEvent
    let sequence: [TriggerActionPayload]
    let variables: [String: String]
    let scope: String
    var projectRoot: String?
    var projectName: String?

    init(from payload: TriggerInfoPayload) {
        self.id = payload.name
        self.name = payload.name
        self.description = payload.description
        self.event = TriggerEvent(rawValue: payload.on) ?? .agentIdle
        self.sequence = payload.sequence
        self.variables = payload.variables
        self.scope = payload.scope
    }
}
