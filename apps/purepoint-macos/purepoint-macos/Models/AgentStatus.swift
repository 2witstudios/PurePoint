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
    case suspended

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
        case .suspended: .orange
        }
    }

    var nsColor: NSColor {
        switch self {
        case .running:   .systemGreen
        case .idle:      .systemMint
        case .completed: .systemBlue
        case .failed:    .systemRed
        case .killed:    .systemOrange
        case .spawning:  .systemYellow
        case .waiting:   .systemGray
        case .lost:      .systemGray
        case .suspended: .systemOrange
        }
    }

    var isTerminal: Bool {
        switch self {
        case .completed, .failed, .killed, .lost: true
        default: false
        }
    }
}
