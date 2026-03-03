import Foundation

enum SettingsSection: String, CaseIterable, Identifiable {
    case general
    case hotkeys
    case display
    case about

    var id: String { rawValue }

    var title: String {
        switch self {
        case .general: "General"
        case .hotkeys: "Hotkeys"
        case .display: "Display"
        case .about: "About"
        }
    }

    var icon: String {
        switch self {
        case .general: "gear"
        case .hotkeys: "keyboard"
        case .display: "paintbrush"
        case .about: "info.circle"
        }
    }
}
