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
    @ObservationIgnored private let defaults: UserDefaults

    // MARK: - General

    var restoreProjectsOnLaunch: Bool = true {
        didSet { defaults.set(restoreProjectsOnLaunch, forKey: "PP_restoreProjectsOnLaunch") }
    }

    var launchAtLogin: Bool = false {
        didSet { defaults.set(launchAtLogin, forKey: "PP_launchAtLogin") }
    }

    var usePointGuardNames: Bool = true {
        didSet { defaults.set(usePointGuardNames, forKey: "PP_usePointGuardNames") }
    }

    var commandPaletteOrder: [String] = [] {
        didSet { defaults.set(commandPaletteOrder, forKey: "PP_commandPaletteOrder") }
    }

    /// Swap visible neighbors while retaining preferences for entries unavailable in this project.
    func moveCommandPaletteItem(_ id: String, by offset: Int, items: [CommandPaletteItem]) {
        let visibleIDs = items.map(\.id)
        guard let index = visibleIDs.firstIndex(of: id),
            visibleIDs.indices.contains(index + offset)
        else { return }

        var seen: Set<String> = []
        var order = (commandPaletteOrder + visibleIDs).filter { seen.insert($0).inserted }
        guard let source = order.firstIndex(of: id),
            let destination = order.firstIndex(of: visibleIDs[index + offset])
        else { return }
        order.swapAt(source, destination)
        commandPaletteOrder = order
    }

    // MARK: - Display

    var appearance: AppAppearance = .system {
        didSet { defaults.set(appearance.rawValue, forKey: "PP_appearance") }
    }

    var terminalFontSize: CGFloat = 13 {
        didSet { defaults.set(terminalFontSize, forKey: "PP_terminalFontSize") }
    }

    var gridGap: CGFloat = 1 {
        didSet { defaults.set(gridGap, forKey: "PP_gridGap") }
    }

    // MARK: - Init

    init(defaults: UserDefaults = .standard) {
        self.defaults = defaults
        if defaults.object(forKey: "PP_usePointGuardNames") != nil {
            usePointGuardNames = defaults.bool(forKey: "PP_usePointGuardNames")
        }
        commandPaletteOrder = defaults.stringArray(forKey: "PP_commandPaletteOrder") ?? []

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
