import Foundation
import Observation
import SwiftUI

enum AppAppearance: String, CaseIterable {
    case system
    case dark
    case light

    var label: String {
        switch self {
        case .system: "System"
        case .dark: "Dark"
        case .light: "Light"
        }
    }

    var colorScheme: ColorScheme? {
        switch self {
        case .system: nil
        case .dark: .dark
        case .light: .light
        }
    }
}

@Observable
@MainActor
final class SettingsState {
    // MARK: - General

    var restoreProjectsOnLaunch: Bool = true {
        didSet { UserDefaults.standard.set(restoreProjectsOnLaunch, forKey: "PP_restoreProjectsOnLaunch") }
    }

    var launchAtLogin: Bool = false {
        didSet { UserDefaults.standard.set(launchAtLogin, forKey: "PP_launchAtLogin") }
    }

    // MARK: - Display

    var appearance: AppAppearance = .system {
        didSet { UserDefaults.standard.set(appearance.rawValue, forKey: "PP_appearance") }
    }

    var terminalFontSize: CGFloat = 13 {
        didSet { UserDefaults.standard.set(terminalFontSize, forKey: "PP_terminalFontSize") }
    }

    var gridGap: CGFloat = 1 {
        didSet { UserDefaults.standard.set(gridGap, forKey: "PP_gridGap") }
    }

    // MARK: - Init

    init() {
        let defaults = UserDefaults.standard

        if defaults.object(forKey: "PP_restoreProjectsOnLaunch") != nil {
            restoreProjectsOnLaunch = defaults.bool(forKey: "PP_restoreProjectsOnLaunch")
        }
        if defaults.object(forKey: "PP_launchAtLogin") != nil {
            launchAtLogin = defaults.bool(forKey: "PP_launchAtLogin")
        }
        if let raw = defaults.string(forKey: "PP_appearance"),
            let v = AppAppearance(rawValue: raw)
        {
            appearance = v
        }
        if defaults.object(forKey: "PP_terminalFontSize") != nil {
            terminalFontSize = defaults.double(forKey: "PP_terminalFontSize")
        }
        if defaults.object(forKey: "PP_gridGap") != nil {
            gridGap = defaults.double(forKey: "PP_gridGap")
        }

        // Validate loaded values
        if terminalFontSize < 8 || terminalFontSize > 72 { terminalFontSize = 13 }
        if gridGap < 0 { gridGap = 1 }
    }
}
