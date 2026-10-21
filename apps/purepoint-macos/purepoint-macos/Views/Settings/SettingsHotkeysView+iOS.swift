#if os(iOS)
import SwiftUI
import UIKit

/// What a key press means to the hotkey recorder.
enum HotkeyRecorderInput: Equatable {
    /// Bare Escape: stop recording.
    case cancel
    /// A press without Command, Control or Option: ignored, as on the Mac.
    case ignored
    case binding(KeyBinding)

    static func from(
        keyCode: UIKeyboardHIDUsage, charactersIgnoringModifiers: String, modifierFlags: UIKeyModifierFlags
    ) -> HotkeyRecorderInput {
        if keyCode == SpecialKey.escape.keyboardUsage
            && modifierFlags.intersection(KeyBinding.relevantModifierFlags).isEmpty
        {
            return .cancel
        }
        guard
            let binding = KeyBinding.from(
                keyCode: keyCode, charactersIgnoringModifiers: charactersIgnoringModifiers,
                modifierFlags: modifierFlags)
        else { return .ignored }
        return .binding(binding)
    }
}

/// iOS stand-in for the Mac recorder's `NSEvent` monitor: an invisible view that holds the
/// keyboard while a row records, and reports every hardware key press to `onInput`.
struct HotkeyRecorderView: UIViewRepresentable {
    let keyBindingState: KeyBindingState
    let onInput: (HotkeyRecorderInput) -> Void

    func makeUIView(context: Context) -> HotkeyRecorderResponder {
        let view = HotkeyRecorderResponder()
        view.keyBindingState = keyBindingState
        view.onInput = onInput
        return view
    }

    func updateUIView(_ view: HotkeyRecorderResponder, context: Context) {
        view.keyBindingState = keyBindingState
        view.onInput = onInput
    }

    static func dismantleUIView(_ view: HotkeyRecorderResponder, coordinator: ()) {
        view.resignFirstResponder()
    }
}

/// The first responder while recording. Most presses arrive in `pressesBegan`. A combination
/// that is already bound is a key command somewhere up the responder chain or in the menu bar,
/// which UIKit would run before delivering the press, so the recorder registers every current
/// binding as its own key command: as the first responder its commands match first.
final class HotkeyRecorderResponder: UIView {
    var keyBindingState: KeyBindingState?
    var onInput: ((HotkeyRecorderInput) -> Void)?

    override var canBecomeFirstResponder: Bool { true }

    override func didMoveToWindow() {
        super.didMoveToWindow()
        if window != nil { becomeFirstResponder() }
    }

    override var keyCommands: [UIKeyCommand]? {
        guard let state = keyBindingState else { return nil }
        let bindings = Set(HotkeyAction.allCases.map { state.binding(for: $0) })
        return bindings.map { $0.keyCommand(action: #selector(recordKeyCommand(_:))) }
    }

    @objc private func recordKeyCommand(_ sender: UIKeyCommand) {
        guard
            let binding = KeyBinding.from(
                keyCommandInput: sender.input ?? "", modifierFlags: sender.modifierFlags)
        else { return }
        onInput?(.binding(binding))
    }

    override func pressesBegan(_ presses: Set<UIPress>, with event: UIPressesEvent?) {
        guard let key = presses.first(where: { $0.key != nil })?.key else {
            super.pressesBegan(presses, with: event)
            return
        }
        // A lone modifier press is part of a combination still being typed.
        guard !Self.modifierKeys.contains(key.keyCode) else { return }
        onInput?(
            HotkeyRecorderInput.from(
                keyCode: key.keyCode, charactersIgnoringModifiers: key.charactersIgnoringModifiers,
                modifierFlags: key.modifierFlags))
    }

    // Every press is the recorder's while it records; nothing reaches the views behind it.
    override func pressesEnded(_ presses: Set<UIPress>, with event: UIPressesEvent?) {}
    override func pressesChanged(_ presses: Set<UIPress>, with event: UIPressesEvent?) {}
    override func pressesCancelled(_ presses: Set<UIPress>, with event: UIPressesEvent?) {}

    private static let modifierKeys: Set<UIKeyboardHIDUsage> = [
        .keyboardLeftGUI, .keyboardRightGUI, .keyboardLeftShift, .keyboardRightShift,
        .keyboardLeftAlt, .keyboardRightAlt, .keyboardLeftControl, .keyboardRightControl,
        .keyboardCapsLock,
    ]
}
#endif
