import AppKit

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

    /// Normalize legacy status to the three observable states
    var normalized: AgentStatus {
        switch self {
        case .streaming, .running, .spawning: .streaming
        case .waiting, .idle, .suspended: .waiting
        case .broken, .completed, .failed,
            .killed, .lost:
            .broken
        }
    }

    var nsColor: NSColor {
        switch normalized {
        case .streaming: .systemGreen
        case .waiting: .systemCyan
        case .broken: .systemRed
        default: .systemGray
        }
    }

    var isAlive: Bool {
        switch normalized {
        case .streaming, .waiting: true
        default: false
        }
    }
}
