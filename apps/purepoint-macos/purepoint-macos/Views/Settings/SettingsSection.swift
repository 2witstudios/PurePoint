import Foundation

enum SettingsSection: String, CaseIterable, Identifiable {
    case general
    case pointGuard
    case agents
    case hotkeys
    case display
    // About holds the Sparkle update check, which the iPad does not have (REQ-IPAD-040).
    #if os(macOS)
    case about
    #endif

    var id: String { rawValue }

    var title: String {
        switch self {
        case .general: "General"
        case .pointGuard: "Point Guard"
        case .agents: "Agents"
        case .hotkeys: "Hotkeys"
        case .display: "Display"
        #if os(macOS)
        case .about: "About"
        #endif
        }
    }

    var icon: String {
        switch self {
        case .general: "gear"
        case .pointGuard: "terminal"
        case .agents: "cpu"
        case .hotkeys: "keyboard"
        case .display: "paintbrush"
        #if os(macOS)
        case .about: "info.circle"
        #endif
        }
    }
}
