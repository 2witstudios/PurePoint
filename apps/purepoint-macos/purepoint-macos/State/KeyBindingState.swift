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
        // Load overrides from UserDefaults
        var loadedOverrides: [HotkeyAction: KeyBinding] = [:]
        if let data = UserDefaults.standard.data(forKey: Self.userDefaultsKey),
            let saved = try? JSONDecoder().decode([String: KeyBinding].self, from: data)
        {
            for (rawAction, binding) in saved {
                if let action = HotkeyAction(rawValue: rawAction) {
                    loadedOverrides[action] = binding
                }
            }
        }

        bindings = Self.resolve(defaults: Self.defaultBindings, overrides: loadedOverrides)
        overrides = loadedOverrides
    }

    // MARK: - Resolution

    static var defaultBindings: [HotkeyAction: KeyBinding] {
        Dictionary(uniqueKeysWithValues: HotkeyAction.allCases.map { ($0, $0.defaultBinding) })
    }

    /// The bindings in effect: every saved override wins, and an action still on a default that an
    /// override has claimed (e.g. a newly added action whose default the user had already given to
    /// another action) takes a default some override gave up instead — the same swap the Settings
    /// "Override" button makes. Defaults are unique, so there is always a freed default for each
    /// displaced action and no two actions share a binding.
    static func resolve(
        defaults: [HotkeyAction: KeyBinding], overrides: [HotkeyAction: KeyBinding]
    ) -> [HotkeyAction: KeyBinding] {
        let claimed = Set(overrides.values)
        var freed = HotkeyAction.allCases.compactMap { action -> KeyBinding? in
            guard overrides[action] != nil, let binding = defaults[action], !claimed.contains(binding) else {
                return nil
            }
            return binding
        }

        var result = overrides
        for action in HotkeyAction.allCases where overrides[action] == nil {
            guard let binding = defaults[action] else { continue }
            if !claimed.contains(binding) {
                result[action] = binding
            } else if !freed.isEmpty {
                result[action] = freed.removeFirst()
            }
        }
        return result
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
        overrides[action] = binding
        bindings = Self.resolve(defaults: Self.defaultBindings, overrides: overrides)
        persist()
    }

    func resetBinding(for action: HotkeyAction) {
        overrides.removeValue(forKey: action)
        bindings = Self.resolve(defaults: Self.defaultBindings, overrides: overrides)
        persist()
    }

    func resetAllBindings() {
        overrides.removeAll()
        bindings = Self.defaultBindings
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
