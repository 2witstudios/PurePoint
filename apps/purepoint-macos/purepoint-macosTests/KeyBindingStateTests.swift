import Foundation
import Testing

@testable import PurePoint

/// A saved shortcut always wins, and no two actions ever end up sharing one — even when a new
/// action's default is a shortcut the user had already given to something else.
@MainActor
struct KeyBindingStateTests {
    private let workspaceToggle = KeyBinding(key: .character("\\"), modifiers: [.command, .option])
    private let chatToggle = KeyBinding(key: .character("s"), modifiers: [.command, .shift])

    private func expectNoSharedBindings(_ bindings: [HotkeyAction: KeyBinding]) {
        #expect(Set(bindings.values).count == bindings.count)
    }

    @Test func givenTheDefaultsShouldGiveEveryActionItsOwnShortcut() {
        let defaults = KeyBindingState.defaultBindings

        #expect(defaults.count == HotkeyAction.allCases.count)
        expectNoSharedBindings(defaults)
    }

    @Test func givenNoSavedShortcutsShouldUseEveryDefault() {
        let resolved = KeyBindingState.resolve(defaults: KeyBindingState.defaultBindings, overrides: [:])

        #expect(resolved == KeyBindingState.defaultBindings)
    }

    @Test func givenASavedShortcutOnANewActionsDefaultShouldKeepItAndMoveTheNewActionOff() {
        let resolved = KeyBindingState.resolve(
            defaults: KeyBindingState.defaultBindings,
            overrides: [.toggleChatSidebar: workspaceToggle]
        )

        #expect(resolved[.toggleChatSidebar] == workspaceToggle)
        #expect(resolved[.toggleWorkspaceSidebar] == chatToggle)
        #expect(resolved.count == HotkeyAction.allCases.count)
        expectNoSharedBindings(resolved)
    }

    @Test func givenChainedSavedShortcutsShouldStillGiveEveryActionItsOwnShortcut() {
        let newAgentDefault = HotkeyAction.newAgent.defaultBinding
        let openProjectDefault = HotkeyAction.openProject.defaultBinding

        let resolved = KeyBindingState.resolve(
            defaults: KeyBindingState.defaultBindings,
            overrides: [.newAgent: workspaceToggle, .openProject: newAgentDefault]
        )

        #expect(resolved[.newAgent] == workspaceToggle)
        #expect(resolved[.openProject] == newAgentDefault)
        #expect(resolved[.toggleWorkspaceSidebar] == openProjectDefault)
        #expect(resolved.count == HotkeyAction.allCases.count)
        expectNoSharedBindings(resolved)
    }

    @Test func givenASavedShortcutThatIsNobodysDefaultShouldLeaveTheOtherDefaultsAlone() {
        let custom = KeyBinding(key: .character("k"), modifiers: [.command, .control])

        let resolved = KeyBindingState.resolve(
            defaults: KeyBindingState.defaultBindings,
            overrides: [.toggleWorkspaceSidebar: custom]
        )

        var expected = KeyBindingState.defaultBindings
        expected[.toggleWorkspaceSidebar] = custom
        #expect(resolved == expected)
    }
}
