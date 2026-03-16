import AppKit

enum AgentStatus: String, CaseIterable, Codable, Sendable {
    // Current observable states
    case running
    case broken

    // Legacy values — exist solely for backward compat with old manifests.
    // The current daemon only writes running/broken. These cases
    // will never appear in newly-written manifests and can be removed once
    // we drop support for pre-v1 manifest files.
    case streaming
    case waiting
    case idle
    case completed
    case failed
    case killed
    case spawning
    case lost
    case suspended

    /// Normalize legacy status to the two observable states
    var normalized: AgentStatus {
        switch self {
        case .running, .streaming, .waiting, .spawning, .idle, .suspended: .running
        case .broken, .completed, .failed, .killed, .lost: .broken
        }
    }

    var nsColor: NSColor {
        switch normalized {
        case .running: .systemGreen
        case .broken: .systemRed
        default: .systemGray
        }
    }

    var isAlive: Bool {
        switch normalized {
        case .running: true
        default: false
        }
    }
}
