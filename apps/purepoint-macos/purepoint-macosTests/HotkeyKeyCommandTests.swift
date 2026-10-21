#if os(iOS)
import Foundation
import SwiftUI
import Testing
import UIKit

@testable import PurePoint

/// IPAD-6.11: the Mac's key bindings as root `UIKeyCommand`s that post `.hotkeyAction`.
@MainActor
@Suite(.serialized) struct HotkeyKeyCommandTests {

    private func freshState() -> KeyBindingState {
        let state = KeyBindingState()
        state.resetAllBindings()
        return state
    }

    // MARK: - Binding to key command

    @Test func characterBindingMapsToItsInputAndModifiers() {
        let command = KeyBinding(key: .character("S"), modifiers: [.command, .shift])
            .keyCommand(action: #selector(UIWindow.performHotkeyKeyCommand(_:)))

        #expect(command.input == "s")
        #expect(command.modifierFlags == [.command, .shift])
        #expect(command.wantsPriorityOverSystemBehavior)
    }

    @Test func specialKeyBindingMapsToTheUIKitInput() {
        let up = KeyBinding(key: .special(.upArrow), modifiers: [.command, .option])
        #expect(up.keyCommandInput == UIKeyCommand.inputUpArrow)
        #expect(up.keyModifierFlags == [.command, .alternate])
        #expect(KeyBinding(key: .special(.escape), modifiers: [.control]).keyCommandInput == UIKeyCommand.inputEscape)
        #expect(KeyBinding(key: .special(.return), modifiers: [.control]).keyCommandInput == "\r")
    }

    @Test func everyDefaultBindingReadsBackFromItsKeyCommand() {
        for action in HotkeyAction.allCases {
            let binding = action.defaultBinding
            let read = KeyBinding.from(keyCommandInput: binding.keyCommandInput, modifierFlags: binding.keyModifierFlags)
            #expect(read == binding, "\(action)")
        }
    }

    // MARK: - Monitor commands

    @Test func registersOnlyTheMonitorHandledActions() {
        let commands = HotkeyMonitor.keyCommands(for: freshState())
        let actions = commands.compactMap(HotkeyMonitor.action(for:))

        #expect(Set(actions) == Set(HotkeyAction.allCases.filter(\.isMonitorHandled)))
        #expect(actions.count == commands.count)
        let allPrioritized = commands.allSatisfy { $0.wantsPriorityOverSystemBehavior }
        #expect(allPrioritized)
    }

    @Test func eachCommandCarriesItsActionsBinding() {
        let state = freshState()
        for command in HotkeyMonitor.keyCommands(for: state) {
            let action = try! #require(HotkeyMonitor.action(for: command))
            #expect(command.input == state.binding(for: action).keyCommandInput)
            #expect(command.modifierFlags == state.binding(for: action).keyModifierFlags)
        }
    }

    @Test func standsDownWhileRecording() {
        let state = freshState()
        state.recordingAction = .focusSidebar
        #expect(HotkeyMonitor.keyCommands(for: state).isEmpty)
    }

    @Test func rebindingChangesTheCommand() {
        let state = freshState()
        defer { state.resetAllBindings() }
        state.setBinding(KeyBinding(key: .character("j"), modifiers: [.control, .option]), for: .toggleChatSidebar)

        let command = HotkeyMonitor.keyCommands(for: state).first { HotkeyMonitor.action(for: $0) == .toggleChatSidebar }
        #expect(command?.input == "j")
        #expect(command?.modifierFlags == [.control, .alternate])
    }

    @Test func firingACommandPostsTheSameNotificationAsTheMac() async {
        let command = try! #require(
            HotkeyMonitor.keyCommands(for: freshState()).first { HotkeyMonitor.action(for: $0) == .navSchedule })
        var received: HotkeyAction?
        let observer = NotificationCenter.default.addObserver(forName: .hotkeyAction, object: nil, queue: nil) {
            received = $0.userInfo?["action"] as? HotkeyAction
        }
        defer { NotificationCenter.default.removeObserver(observer) }

        UIWindow().performHotkeyKeyCommand(command)

        #expect(received == .navSchedule)
    }

    // MARK: - Registration on the app's windows

    private func appRootController() throws -> UIViewController {
        let window = try #require(
            UIApplication.shared.connectedScenes.compactMap { $0 as? UIWindowScene }.flatMap(\.windows)
                .first { $0.rootViewController != nil })
        return try #require(window.rootViewController)
    }

    private func hasCommand(_ controller: UIViewController, for binding: KeyBinding) -> Bool {
        (controller.keyCommands ?? []).contains {
            $0.input == binding.keyCommandInput && $0.modifierFlags == binding.keyModifierFlags
                && HotkeyMonitor.action(for: $0) != nil
        }
    }

    @Test func installsOnTheWindowRootAndFollowsRebinding() async throws {
        let root = try appRootController()
        let state = freshState()
        defer { state.resetAllBindings() }
        // Combinations no default uses, so the host app's own monitor cannot account for them.
        let first = KeyBinding(key: .character("y"), modifiers: [.command, .control])
        let second = KeyBinding(key: .character("u"), modifiers: [.command, .control])
        state.setBinding(first, for: .toggleChatSidebar)
        let monitor = HotkeyMonitor()
        monitor.keyBindingState = state
        monitor.start()
        defer { monitor.stop() }

        // The SwiftUI hosting controller keeps the added commands in its responder chain list.
        #expect(hasCommand(root, for: first))

        state.setBinding(second, for: .toggleChatSidebar)
        for _ in 0..<50 where !hasCommand(root, for: second) {
            try await Task.sleep(for: .milliseconds(10))
        }

        #expect(hasCommand(root, for: second))
        #expect(!hasCommand(root, for: first))

        monitor.stop()
        #expect(!hasCommand(root, for: second))
    }
}
#endif
