import AppKit
import SwiftUI

// MARK: - HotkeyCategory

enum HotkeyCategory: String, CaseIterable, Identifiable {
    case application
    case navigation
    case panes
    case tabs
    case chat

    var id: String { rawValue }

    var displayName: String {
        switch self {
        case .application: "Application"
        case .navigation: "Navigation"
        case .panes: "Panes"
        case .tabs: "Tabs"
        case .chat: "Chat"
        }
    }
}

// MARK: - HotkeyAction

enum HotkeyAction: String, CaseIterable, Codable, Identifiable {
    // Application
    case newAgent
    case openProject
    case settings
    case closeAgent

    // Navigation
    case focusSidebar
    case focusContent
    case toggleSidebar
    case navDashboard
    case navAgents
    case navSchedule

    // Panes
    case splitBelow
    case splitRight
    case closePane
    case focusUp
    case focusDown
    case focusLeft
    case focusRight

    // Tabs
    case newTab
    case nextTab
    case previousTab
    case selectTab1
    case selectTab2
    case selectTab3
    case selectTab4
    case selectTab5
    case selectTab6
    case selectTab7
    case selectTab8
    case selectLastTab
    case breakTab

    // Chat
    case toggleChatSidebar

    var id: String { rawValue }

    var displayName: String {
        switch self {
        case .newAgent: "New Agent"
        case .openProject: "Open Project"
        case .settings: "Settings"
        case .closeAgent: "Close Tab"
        case .focusSidebar: "Focus Sidebar"
        case .focusContent: "Focus Content"
        case .toggleSidebar: "Toggle Sidebar"
        case .navDashboard: "Dashboard"
        case .navAgents: "Agents"
        case .navSchedule: "Schedule"
        case .splitBelow: "Split Below"
        case .splitRight: "Split Right"
        case .closePane: "Close Pane"
        case .focusUp: "Focus Up"
        case .focusDown: "Focus Down"
        case .focusLeft: "Focus Left"
        case .focusRight: "Focus Right"
        case .newTab: "New Tab"
        case .nextTab: "Next Tab"
        case .previousTab: "Previous Tab"
        case .selectTab1, .selectTab2, .selectTab3, .selectTab4,
            .selectTab5, .selectTab6, .selectTab7, .selectTab8:
            "Go to Tab \(tabIndex.map { $0 + 1 } ?? 0)"
        case .selectLastTab: "Go to Last Tab"
        case .breakTab: "Move Tab to New Pane"
        case .toggleChatSidebar: "Toggle Chat Sidebar"
        }
    }

    var category: HotkeyCategory {
        switch self {
        case .newAgent, .openProject, .settings: .application
        case .focusSidebar, .focusContent, .toggleSidebar, .navDashboard, .navAgents, .navSchedule: .navigation
        case .splitBelow, .splitRight, .closePane, .focusUp, .focusDown, .focusLeft, .focusRight: .panes
        case .closeAgent, .newTab, .nextTab, .previousTab, .selectTab1, .selectTab2, .selectTab3, .selectTab4,
            .selectTab5, .selectTab6, .selectTab7, .selectTab8, .selectLastTab, .breakTab:
            .tabs
        case .toggleChatSidebar: .chat
        }
    }

    var defaultBinding: KeyBinding {
        switch self {
        case .newAgent: KeyBinding(key: .character("n"), modifiers: [.command])
        case .openProject: KeyBinding(key: .character("o"), modifiers: [.command])
        case .settings: KeyBinding(key: .character(","), modifiers: [.command])
        case .closeAgent: KeyBinding(key: .character("w"), modifiers: [.command])
        case .focusSidebar: KeyBinding(key: .character("["), modifiers: [.command])
        case .focusContent: KeyBinding(key: .character("]"), modifiers: [.command])
        case .toggleSidebar: KeyBinding(key: .character("\\"), modifiers: [.command])
        case .navDashboard: KeyBinding(key: .character("1"), modifiers: [.control])
        case .navAgents: KeyBinding(key: .character("2"), modifiers: [.control])
        case .navSchedule: KeyBinding(key: .character("3"), modifiers: [.control])
        case .splitBelow: KeyBinding(key: .character("d"), modifiers: [.command])
        case .splitRight: KeyBinding(key: .character("d"), modifiers: [.command, .shift])
        case .closePane: KeyBinding(key: .character("w"), modifiers: [.command, .shift])
        case .focusUp: KeyBinding(key: .special(.upArrow), modifiers: [.command, .option])
        case .focusDown: KeyBinding(key: .special(.downArrow), modifiers: [.command, .option])
        case .focusLeft: KeyBinding(key: .special(.leftArrow), modifiers: [.command, .option])
        case .focusRight: KeyBinding(key: .special(.rightArrow), modifiers: [.command, .option])
        case .newTab: KeyBinding(key: .character("t"), modifiers: [.command])
        case .nextTab: KeyBinding(key: .character("]"), modifiers: [.command, .shift])
        case .previousTab: KeyBinding(key: .character("["), modifiers: [.command, .shift])
        case .selectTab1: KeyBinding(key: .character("1"), modifiers: [.command])
        case .selectTab2: KeyBinding(key: .character("2"), modifiers: [.command])
        case .selectTab3: KeyBinding(key: .character("3"), modifiers: [.command])
        case .selectTab4: KeyBinding(key: .character("4"), modifiers: [.command])
        case .selectTab5: KeyBinding(key: .character("5"), modifiers: [.command])
        case .selectTab6: KeyBinding(key: .character("6"), modifiers: [.command])
        case .selectTab7: KeyBinding(key: .character("7"), modifiers: [.command])
        case .selectTab8: KeyBinding(key: .character("8"), modifiers: [.command])
        case .selectLastTab: KeyBinding(key: .character("9"), modifiers: [.command])
        case .breakTab: KeyBinding(key: .character("d"), modifiers: [.command, .option])
        case .toggleChatSidebar: KeyBinding(key: .character("s"), modifiers: [.command, .shift])
        }
    }

    /// Actions handled by the NSEvent monitor (not SwiftUI .commands menus)
    var isMonitorHandled: Bool {
        switch self {
        case .focusSidebar, .focusContent, .toggleSidebar,
            .navDashboard, .navAgents, .navSchedule,
            .toggleChatSidebar:
            return true
        default:
            return false
        }
    }

    /// The 0-based tab position a "Go to Tab N" action selects.
    var tabIndex: Int? {
        switch self {
        case .selectTab1: 0
        case .selectTab2: 1
        case .selectTab3: 2
        case .selectTab4: 3
        case .selectTab5: 4
        case .selectTab6: 5
        case .selectTab7: 6
        case .selectTab8: 7
        default: nil
        }
    }

    static let selectTabActions: [HotkeyAction] = [
        .selectTab1, .selectTab2, .selectTab3, .selectTab4, .selectTab5, .selectTab6, .selectTab7, .selectTab8,
    ]

    static func actions(for category: HotkeyCategory) -> [HotkeyAction] {
        allCases.filter { $0.category == category }
    }
}

// MARK: - KeyModifier

enum KeyModifier: String, Codable, Hashable, Comparable {
    case control
    case option
    case shift
    case command

    var symbol: String {
        switch self {
        case .control: "\u{2303}"  // ⌃
        case .option: "\u{2325}"  // ⌥
        case .shift: "\u{21E7}"  // ⇧
        case .command: "\u{2318}"  // ⌘
        }
    }

    var eventModifier: EventModifiers {
        switch self {
        case .control: .control
        case .option: .option
        case .shift: .shift
        case .command: .command
        }
    }

    var nsEventFlag: NSEvent.ModifierFlags {
        switch self {
        case .control: .control
        case .option: .option
        case .shift: .shift
        case .command: .command
        }
    }

    /// Sort order for display: ⌃ ⌥ ⇧ ⌘
    private var sortOrder: Int {
        switch self {
        case .control: 0
        case .option: 1
        case .shift: 2
        case .command: 3
        }
    }

    static func < (lhs: KeyModifier, rhs: KeyModifier) -> Bool {
        lhs.sortOrder < rhs.sortOrder
    }
}

// MARK: - SpecialKey

enum SpecialKey: String, Codable, Hashable {
    case upArrow
    case downArrow
    case leftArrow
    case rightArrow
    case escape
    case `return`
    case tab
    case space
    case delete

    var displaySymbol: String {
        switch self {
        case .upArrow: "\u{2191}"  // ↑
        case .downArrow: "\u{2193}"  // ↓
        case .leftArrow: "\u{2190}"  // ←
        case .rightArrow: "\u{2192}"  // →
        case .escape: "\u{238B}"  // ⎋
        case .return: "\u{21A9}"  // ↩
        case .tab: "\u{21E5}"  // ⇥
        case .space: "\u{2423}"  // ␣
        case .delete: "\u{232B}"  // ⌫
        }
    }

    var keyEquivalent: KeyEquivalent {
        switch self {
        case .upArrow: .upArrow
        case .downArrow: .downArrow
        case .leftArrow: .leftArrow
        case .rightArrow: .rightArrow
        case .escape: .escape
        case .return: .return
        case .tab: .tab
        case .space: .space
        case .delete: .delete
        }
    }

    var keyCode: UInt16 {
        switch self {
        case .upArrow: 126
        case .downArrow: 125
        case .leftArrow: 123
        case .rightArrow: 124
        case .escape: 53
        case .return: 36
        case .tab: 48
        case .space: 49
        case .delete: 51
        }
    }
}

// MARK: - KeyBindingKey

enum KeyBindingKey: Codable, Equatable, Hashable {
    case character(Character)
    case special(SpecialKey)

    var displaySymbol: String {
        switch self {
        case .character(let c): String(c).uppercased()
        case .special(let s): s.displaySymbol
        }
    }

    // Codable conformance for Character
    enum CodingKeys: String, CodingKey {
        case type, value
    }

    func encode(to encoder: Encoder) throws {
        var container = encoder.container(keyedBy: CodingKeys.self)
        switch self {
        case .character(let c):
            try container.encode("character", forKey: .type)
            try container.encode(String(c), forKey: .value)
        case .special(let s):
            try container.encode("special", forKey: .type)
            try container.encode(s.rawValue, forKey: .value)
        }
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        let type = try container.decode(String.self, forKey: .type)
        let value = try container.decode(String.self, forKey: .value)
        switch type {
        case "character":
            guard let c = value.first else {
                throw DecodingError.dataCorruptedError(
                    forKey: .value, in: container, debugDescription: "Empty character")
            }
            self = .character(c)
        case "special":
            guard let s = SpecialKey(rawValue: value) else {
                throw DecodingError.dataCorruptedError(
                    forKey: .value, in: container, debugDescription: "Unknown special key")
            }
            self = .special(s)
        default:
            throw DecodingError.dataCorruptedError(forKey: .type, in: container, debugDescription: "Unknown key type")
        }
    }
}

// MARK: - KeyBinding

struct KeyBinding: Codable, Equatable, Hashable {
    let key: KeyBindingKey
    let modifiers: Set<KeyModifier>

    /// Ordered display keys for UI rendering (e.g., ["⌃", "⌘", "D"])
    var displayKeys: [String] {
        let sortedMods = modifiers.sorted()
        return sortedMods.map(\.symbol) + [key.displaySymbol]
    }

    /// SwiftUI KeyEquivalent for `.keyboardShortcut()`
    var keyEquivalent: KeyEquivalent {
        switch key {
        case .character(let c): KeyEquivalent(c)
        case .special(let s): s.keyEquivalent
        }
    }

    /// SwiftUI EventModifiers for `.keyboardShortcut()`
    var eventModifiers: EventModifiers {
        var result: EventModifiers = []
        for mod in modifiers {
            result.insert(mod.eventModifier)
        }
        return result
    }

    /// Check if this binding matches an NSEvent
    func matches(_ event: NSEvent) -> Bool {
        // Check modifiers (mask off irrelevant bits)
        let relevantFlags: NSEvent.ModifierFlags = [.command, .shift, .option, .control]
        let eventMods = event.modifierFlags.intersection(relevantFlags)
        var expectedMods: NSEvent.ModifierFlags = []
        for mod in modifiers {
            expectedMods.insert(mod.nsEventFlag)
        }
        guard eventMods == expectedMods else { return false }

        // Check key
        switch key {
        case .character(let c):
            let chars = event.charactersIgnoringModifiers?.lowercased() ?? ""
            return chars == String(c).lowercased()
        case .special(let s):
            return event.keyCode == s.keyCode
        }
    }

    /// Create from an NSEvent (for recording)
    static func from(event: NSEvent) -> KeyBinding? {
        let relevantFlags: NSEvent.ModifierFlags = [.command, .shift, .option, .control]
        let eventMods = event.modifierFlags.intersection(relevantFlags)

        var modifiers = Set<KeyModifier>()
        if eventMods.contains(.command) { modifiers.insert(.command) }
        if eventMods.contains(.shift) { modifiers.insert(.shift) }
        if eventMods.contains(.option) { modifiers.insert(.option) }
        if eventMods.contains(.control) { modifiers.insert(.control) }

        // Must include at least one modifier (Cmd, Ctrl, or Option)
        guard modifiers.contains(.command) || modifiers.contains(.control) || modifiers.contains(.option) else {
            return nil
        }

        // Determine key
        let bindingKey: KeyBindingKey
        if let special = specialKey(from: event.keyCode) {
            bindingKey = .special(special)
        } else if let chars = event.charactersIgnoringModifiers?.lowercased(), let c = chars.first {
            bindingKey = .character(c)
        } else {
            return nil
        }

        return KeyBinding(key: bindingKey, modifiers: modifiers)
    }

    private static func specialKey(from keyCode: UInt16) -> SpecialKey? {
        for s in [SpecialKey.upArrow, .downArrow, .leftArrow, .rightArrow, .escape, .return, .tab, .space, .delete] {
            if s.keyCode == keyCode { return s }
        }
        return nil
    }
}

// MARK: - Notification

extension Notification.Name {
    static let hotkeyAction = Notification.Name("PP_hotkeyAction")
    static let toggleChatSidebar = Notification.Name("PP_toggleChatSidebar")
}
