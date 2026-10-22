#if os(iOS)
import Foundation
import Testing
import UIKit

@testable import PurePoint

/// IPAD-6.12: the Settings recorder reads hardware key presses as the Mac recorder reads `NSEvent`s.
@MainActor
@Suite struct HotkeyRecorderTests {

    private func input(
        _ keyCode: UIKeyboardHIDUsage, _ chars: String, _ flags: UIKeyModifierFlags
    ) -> HotkeyRecorderInput {
        HotkeyRecorderInput.from(keyCode: keyCode, charactersIgnoringModifiers: chars, modifierFlags: flags)
    }

    @Test func bareEscapeCancels() {
        #expect(input(.keyboardEscape, "\u{1b}", []) == .cancel)
        #expect(input(.keyboardEscape, "\u{1b}", .alphaShift) == .cancel)
    }

    @Test func modifiedEscapeRecords() {
        #expect(input(.keyboardEscape, "\u{1b}", .command) == .binding(KeyBinding(key: .special(.escape), modifiers: [.command])))
    }

    @Test func keysWithoutCommandControlOrOptionAreIgnored() {
        #expect(input(.keyboardA, "a", []) == .ignored)
        #expect(input(.keyboardA, "A", .shift) == .ignored)
    }

    @Test func characterCombinationsRecordLowercased() {
        #expect(
            input(.keyboardK, "K", [.command, .shift])
                == .binding(KeyBinding(key: .character("k"), modifiers: [.command, .shift])))
        #expect(
            input(.keyboard1, "1", [.control, .alternate, .alphaShift])
                == .binding(KeyBinding(key: .character("1"), modifiers: [.control, .option])))
    }

    @Test func specialKeysRecordByKeyCode() {
        let cases: [(UIKeyboardHIDUsage, SpecialKey)] = [
            (.keyboardUpArrow, .upArrow), (.keyboardDownArrow, .downArrow), (.keyboardLeftArrow, .leftArrow),
            (.keyboardRightArrow, .rightArrow), (.keyboardReturnOrEnter, .return), (.keyboardTab, .tab),
            (.keyboardSpacebar, .space), (.keyboardDeleteOrBackspace, .delete),
        ]
        for (code, special) in cases {
            #expect(input(code, "", [.command, .alternate]) == .binding(KeyBinding(key: .special(special), modifiers: [.command, .option])))
        }
    }

    @Test func recorderClaimsEveryBoundCombination() {
        let state = KeyBindingState()
        let responder = HotkeyRecorderResponder()
        responder.keyBindingState = state

        let claimed = Set(
            (responder.keyCommands ?? []).compactMap {
                KeyBinding.from(keyCommandInput: $0.input ?? "", modifierFlags: $0.modifierFlags)
            })
        #expect(claimed == Set(HotkeyAction.allCases.map { state.binding(for: $0) }))
        let allPrioritized = (responder.keyCommands ?? []).allSatisfy { $0.wantsPriorityOverSystemBehavior }
        #expect(allPrioritized)
    }

    @Test func recordedBindingIsSavedToKeyBindingState() {
        let state = KeyBindingState()
        defer { state.resetAllBindings() }
        guard case .binding(let binding) = input(.keyboardJ, "j", [.command, .control]) else {
            Issue.record("expected a binding")
            return
        }
        #expect(state.detectConflict(binding, excluding: .toggleChatSidebar) == nil)
        state.setBinding(binding, for: .toggleChatSidebar)

        #expect(KeyBindingState().binding(for: .toggleChatSidebar) == binding)
    }
}
#endif
