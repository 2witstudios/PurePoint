import SwiftUI

enum AgentStatus: String, CaseIterable {
    case running
    case completed
    case failed
    case killed
    case spawning
    case waiting
    case lost

    var color: Color {
        switch self {
        case .running:   .green
        case .completed: .blue
        case .failed:    .red
        case .killed:    .orange
        case .spawning:  .yellow
        case .waiting:   .gray
        case .lost:      .gray
        }
    }
}
