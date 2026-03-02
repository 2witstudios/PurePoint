import SwiftUI

enum AgentStatus: String, CaseIterable, Codable, Sendable {
    case running
    case idle
    case completed
    case failed
    case killed
    case spawning
    case waiting
    case lost

    var color: Color {
        switch self {
        case .running:   .green
        case .idle:      .mint
        case .completed: .blue
        case .failed:    .red
        case .killed:    .orange
        case .spawning:  .yellow
        case .waiting:   .gray
        case .lost:      .gray
        }
    }

    var isTerminal: Bool {
        switch self {
        case .completed, .failed, .killed, .lost: true
        default: false
        }
    }
}
