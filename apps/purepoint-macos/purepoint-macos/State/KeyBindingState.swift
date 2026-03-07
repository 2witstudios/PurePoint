import Foundation
import Observation
import SwiftUI

@Observable
@MainActor
final class KeyBindingState {
    private static let userDefaultsKey = "PP_customKeyBindings"

    /// Current bindings (defaults merged with user overrides)
    private(set) var bindings: [HotkeyAction: KeyBinding]

    /// Which action is currently being recorded (nil = not recording)
    var recordingAction: HotkeyAction?

    /// User overrides only (persisted)
    private var overrides: [HotkeyAction: KeyBinding] = [:]

    init() {
        // Start with all defaults
        var result: [HotkeyAction: KeyBinding] = [:]
        for action in HotkeyAction.allCases {
            result[action] = action.defaultBinding
        }

        // Load overrides from UserDefaults
        var loadedOverrides: [HotkeyAction: KeyBinding] = [:]
        if let data = UserDefaults.standard.data(forKey: Self.userDefaultsKey),
            let saved = try? JSONDecoder().decode([String: KeyBinding].self, from: data)
        {
            for (rawAction, binding) in saved {
                if let action = HotkeyAction(rawValue: rawAction) {
                    result[action] = binding
                    loadedOverrides[action] = binding
                }
            }
        }

        bindings = result
        overrides = loadedOverrides
    }

    // MARK: - Queries

    func binding(for action: HotkeyAction) -> KeyBinding {
        bindings[action] ?? action.defaultBinding
    }

    func keyEquivalent(for action: HotkeyAction) -> KeyEquivalent {
        binding(for: action).keyEquivalent
    }

    func eventModifiers(for action: HotkeyAction) -> EventModifiers {
        binding(for: action).eventModifiers
    }

    func isCustomized(_ action: HotkeyAction) -> Bool {
        overrides[action] != nil
    }

    /// Find an existing action that uses the same binding, excluding a given action
    func detectConflict(_ binding: KeyBinding, excluding: HotkeyAction) -> HotkeyAction? {
        for (action, existing) in bindings where action != excluding {
            if existing == binding { return action }
        }
        return nil
    }

    /// Reverse lookup: find which action matches an NSEvent
    func action(for event: NSEvent) -> HotkeyAction? {
        for (action, binding) in bindings {
            if binding.matches(event) { return action }
        }
        return nil
    }

    // MARK: - Mutations

    func setBinding(_ binding: KeyBinding, for action: HotkeyAction) {
        bindings[action] = binding
        overrides[action] = binding
        persist()
    }

    func resetBinding(for action: HotkeyAction) {
        bindings[action] = action.defaultBinding
        overrides.removeValue(forKey: action)
        persist()
    }

    func resetAllBindings() {
        overrides.removeAll()
        for action in HotkeyAction.allCases {
            bindings[action] = action.defaultBinding
        }
        persist()
    }

    // MARK: - Persistence

    private func persist() {
        var dict: [String: KeyBinding] = [:]
        for (action, binding) in overrides {
            dict[action.rawValue] = binding
        }
        if let data = try? JSONEncoder().encode(dict) {
            UserDefaults.standard.set(data, forKey: Self.userDefaultsKey)
        }
    }
}
