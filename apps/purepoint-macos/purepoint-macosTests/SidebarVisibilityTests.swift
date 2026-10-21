import Foundation
import Testing

@testable import PurePoint

/// Both sidebars' shown/hidden state outlives a relaunch, and the hotkey that toggles the
/// workspace sidebar is listed and bound like the others.
@MainActor
struct SidebarVisibilityTests {
    /// A private suite, so a test never reaches the real defaults; removed with the test.
    private func makeDefaults() throws -> (defaults: UserDefaults, suite: String) {
        let suite = "pp-sidebar-visibility-\(UUID().uuidString)"
        return (try #require(UserDefaults(suiteName: suite)), suite)
    }

    @Test func givenNothingStoredShouldShowBothSidebars() throws {
        let (defaults, suite) = try makeDefaults()
        defer { UserDefaults(suiteName: suite)?.removePersistentDomain(forName: suite) }

        #expect(SidebarVisibility.isShown(SidebarVisibility.sidebarKey, in: defaults))
        #expect(SidebarVisibility.isShown(SidebarVisibility.workspaceSidebarKey, in: defaults))
    }

    @Test func givenASidebarHiddenShouldStayHiddenAcrossALaunch() throws {
        let (defaults, suite) = try makeDefaults()
        defer { UserDefaults(suiteName: suite)?.removePersistentDomain(forName: suite) }
        SidebarVisibility.toggle(SidebarVisibility.sidebarKey, in: defaults)

        let relaunched = try #require(UserDefaults(suiteName: suite))
        #expect(!SidebarVisibility.isShown(SidebarVisibility.sidebarKey, in: relaunched))
    }

    @Test func givenAToggleTwiceShouldShowTheSidebarAgain() throws {
        let (defaults, suite) = try makeDefaults()
        defer { UserDefaults(suiteName: suite)?.removePersistentDomain(forName: suite) }
        SidebarVisibility.toggle(SidebarVisibility.workspaceSidebarKey, in: defaults)
        SidebarVisibility.toggle(SidebarVisibility.workspaceSidebarKey, in: defaults)

        #expect(SidebarVisibility.isShown(SidebarVisibility.workspaceSidebarKey, in: defaults))
    }

    @Test func givenOneSidebarToggledShouldLeaveTheOtherAlone() throws {
        let (defaults, suite) = try makeDefaults()
        defer { UserDefaults(suiteName: suite)?.removePersistentDomain(forName: suite) }
        SidebarVisibility.toggle(SidebarVisibility.workspaceSidebarKey, in: defaults)

        #expect(SidebarVisibility.isShown(SidebarVisibility.sidebarKey, in: defaults))
    }

    @Test func givenTheWorkspaceSidebarHotkeyShouldBeListedUnderNavigation() {
        #expect(HotkeyAction.actions(for: .navigation).contains(.toggleWorkspaceSidebar))
        #expect(HotkeyAction.toggleWorkspaceSidebar.displayName == "Toggle Workspace Sidebar")
    }

    @Test func givenDefaultBindingsShouldNotClashWithEachOther() {
        let bindings = HotkeyAction.allCases.map(\.defaultBinding)
        #expect(Set(bindings).count == bindings.count)
        #expect(
            HotkeyAction.toggleWorkspaceSidebar.defaultBinding
                == KeyBinding(key: .character("\\"), modifiers: [.command, .option]))
    }
}
