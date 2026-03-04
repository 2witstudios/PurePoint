import SwiftUI

enum AgentStatus: String, CaseIterable, Codable, Sendable {
    // New observable states
    case streaming
    case waiting
    case broken

    // Legacy values — exist solely for backward compat with old manifests.
    // The current daemon only writes streaming/waiting/broken. These cases
    // will never appear in newly-written manifests and can be removed once
    // we drop support for pre-v1 manifest files.
    case running
    case idle
    case completed
    case failed
    case killed
    case spawning
    case lost
    case suspended

    var color: Color {
        switch self {
        case .streaming, .running, .spawning: .green
        case .waiting, .idle, .suspended:     .cyan
        case .completed:                      .secondary
        case .broken, .failed,
             .killed, .lost:                  .red
        }
    }

    var nsColor: NSColor {
        switch self {
        case .streaming, .running, .spawning: .systemGreen
        case .waiting, .idle, .suspended:     .systemCyan
        case .completed:                      .systemGray
        case .broken, .failed,
             .killed, .lost:                  .systemRed
        }
    }

    var isAlive: Bool {
        switch self {
        case .streaming, .waiting, .running, .idle, .spawning, .suspended: true
        case .broken, .completed, .failed, .killed, .lost: false
        }
    }

    /// Normalize legacy status to the three observable states
    var normalized: AgentStatus {
        switch self {
        case .streaming, .running, .spawning: .streaming
        case .waiting, .idle, .suspended:     .waiting
        case .broken, .completed, .failed,
             .killed, .lost:                  .broken
        }
    }
}
