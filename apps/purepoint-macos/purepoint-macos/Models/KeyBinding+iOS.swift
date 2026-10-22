#if os(iOS)
import UIKit

// The iOS side of `KeyBinding`: the `UIKeyCommand` a binding registers as, and the binding a
// hardware key press records as. These replace the Mac's `NSEvent` matching (`matches(_:)`,
// `from(event:)`); the rules are the same.

extension KeyModifier {
    var keyModifierFlag: UIKeyModifierFlags {
        switch self {
        case .control: .control
        case .option: .alternate
        case .shift: .shift
        case .command: .command
        }
    }
}

extension SpecialKey {
    /// The `UIKeyCommand` input for this key.
    var keyCommandInput: String {
        switch self {
        case .upArrow: UIKeyCommand.inputUpArrow
        case .downArrow: UIKeyCommand.inputDownArrow
        case .leftArrow: UIKeyCommand.inputLeftArrow
        case .rightArrow: UIKeyCommand.inputRightArrow
        case .escape: UIKeyCommand.inputEscape
        case .return: "\r"
        case .tab: "\t"
        case .space: " "
        case .delete: UIKeyCommand.inputDelete
        }
    }

    /// The hardware key code, as the Mac's `keyCode` identifies the key on an `NSEvent`.
    var keyboardUsage: UIKeyboardHIDUsage {
        switch self {
        case .upArrow: .keyboardUpArrow
        case .downArrow: .keyboardDownArrow
        case .leftArrow: .keyboardLeftArrow
        case .rightArrow: .keyboardRightArrow
        case .escape: .keyboardEscape
        case .return: .keyboardReturnOrEnter
        case .tab: .keyboardTab
        case .space: .keyboardSpacebar
        case .delete: .keyboardDeleteOrBackspace
        }
    }

    static let all: [SpecialKey] = [.upArrow, .downArrow, .leftArrow, .rightArrow, .escape, .return, .tab, .space, .delete]
}

extension KeyBinding {
    /// The modifiers the Mac matches on; caps lock and the numeric pad flag are ignored.
    static let relevantModifierFlags: UIKeyModifierFlags = [.command, .shift, .alternate, .control]

    var keyCommandInput: String {
        switch key {
        case .character(let c): String(c).lowercased()
        case .special(let s): s.keyCommandInput
        }
    }

    var keyModifierFlags: UIKeyModifierFlags {
        var flags: UIKeyModifierFlags = []
        for mod in modifiers {
            flags.insert(mod.keyModifierFlag)
        }
        return flags
    }

    /// A key command for this binding. It takes priority over a text view's own handling of
    /// the same keys (the terminal's arrow keys), as the Mac's event monitor runs before the
    /// first responder sees the key.
    func keyCommand(action: Selector, propertyList: Any? = nil, title: String = "") -> UIKeyCommand {
        let command = UIKeyCommand(
            title: title, action: action, input: keyCommandInput, modifierFlags: keyModifierFlags,
            propertyList: propertyList)
        command.wantsPriorityOverSystemBehavior = true
        return command
    }

    /// The binding a key press records as, or nil for a bare key: like the Mac recorder, a
    /// binding needs Command, Control or Option.
    static func from(
        keyCode: UIKeyboardHIDUsage, charactersIgnoringModifiers: String, modifierFlags: UIKeyModifierFlags
    ) -> KeyBinding? {
        let eventMods = modifierFlags.intersection(relevantModifierFlags)

        var modifiers = Set<KeyModifier>()
        if eventMods.contains(.command) { modifiers.insert(.command) }
        if eventMods.contains(.shift) { modifiers.insert(.shift) }
        if eventMods.contains(.alternate) { modifiers.insert(.option) }
        if eventMods.contains(.control) { modifiers.insert(.control) }

        guard modifiers.contains(.command) || modifiers.contains(.control) || modifiers.contains(.option) else {
            return nil
        }

        let bindingKey: KeyBindingKey
        if let special = SpecialKey.all.first(where: { $0.keyboardUsage == keyCode }) {
            bindingKey = .special(special)
        } else if let c = charactersIgnoringModifiers.lowercased().first {
            bindingKey = .character(c)
        } else {
            return nil
        }

        return KeyBinding(key: bindingKey, modifiers: modifiers)
    }

    /// Create from a hardware key press (for recording).
    static func from(key: UIKey) -> KeyBinding? {
        from(
            keyCode: key.keyCode, charactersIgnoringModifiers: key.charactersIgnoringModifiers,
            modifierFlags: key.modifierFlags)
    }

    /// The binding a fired key command stands for: its input and modifiers read back.
    static func from(keyCommandInput input: String, modifierFlags: UIKeyModifierFlags) -> KeyBinding? {
        let mods = modifierFlags.intersection(relevantModifierFlags)
        if let special = SpecialKey.all.first(where: { $0.keyCommandInput == input }) {
            return from(keyCode: special.keyboardUsage, charactersIgnoringModifiers: "", modifierFlags: mods)
        }
        return from(keyCode: .keyboardErrorUndefined, charactersIgnoringModifiers: input, modifierFlags: mods)
    }
}
#endif
