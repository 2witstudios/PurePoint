import SwiftUI

/// Where the window's two sidebars keep their shown/hidden state, so each survives a relaunch.
/// A sidebar the user has never toggled is shown.
enum SidebarVisibility {
    /// The machines and projects outline on the left.
    static let sidebarKey = "PP_showSidebar"
    /// The workspace's Changes/Files/Point Guard panel on the right. The key predates the `PP_` prefix.
    static let workspaceSidebarKey = "showWorkspaceFilesSidebar"

    static func isShown(_ key: String, in defaults: UserDefaults = .standard) -> Bool {
        defaults.object(forKey: key) as? Bool ?? true
    }

    /// For callers outside a view, such as menu commands; `@AppStorage` on the same key sees the change.
    static func toggle(_ key: String, in defaults: UserDefaults = .standard) {
        defaults.set(!isShown(key, in: defaults), forKey: key)
    }

    /// The sidebar a hotkey action shows or hides, if it is one of the two toggles.
    static func key(toggledBy action: HotkeyAction) -> String? {
        switch action {
        case .toggleSidebar: sidebarKey
        case .toggleWorkspaceSidebar: workspaceSidebarKey
        default: nil
        }
    }

    /// Toggles the sidebar `action` names, animated, and says whether it named one. Called once
    /// per command rather than from each window: two windows each flipping the shared setting
    /// would cancel out.
    @discardableResult
    static func toggle(for action: HotkeyAction, in defaults: UserDefaults = .standard) -> Bool {
        guard let key = key(toggledBy: action) else { return false }
        withAnimation { toggle(key, in: defaults) }
        return true
    }
}
