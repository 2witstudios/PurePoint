#if os(macOS)
import AppKit
#else
import UIKit
#endif

nonisolated enum Theme {
    // MARK: - Text
    static let primaryText = adaptive(dark: (0.85, 0.85, 0.87, 1.0), light: (0.10, 0.10, 0.12, 1.0))

    // MARK: - Panes
    /// Behind a pane's tab strip — a step darker than the terminal, so the active tab
    /// (drawn in the terminal's color) reads as attached to the content below it. Not
    /// adaptive: the terminal is dark in both appearances, and the strip belongs to it.
    static let tabBarBackground = NSColor(srgbRed: 0.08, green: 0.08, blue: 0.09, alpha: 1.0)

    // MARK: - Point Guard Chat
    /// In dark mode the chat sits on the terminal's color so its active tab reads as attached,
    /// with the history rail a step darker like the tab strip. Light mode keeps system surfaces.
    static let chatBackground = darkOnly((0.11, 0.11, 0.12), light: .windowBackgroundColor)
    static let chatRailBackground = darkOnly((0.08, 0.08, 0.09), light: .controlBackgroundColor)
    static let composerBackground = darkOnly((0.137, 0.137, 0.153), light: NSColor.black.withAlphaComponent(0.045))
    static let composerShadow = darkOnly((0, 0, 0), alpha: 0.3, light: .clear)
    static let userBubbleBackground = darkOnly((0.149, 0.149, 0.165), light: NSColor.black.withAlphaComponent(0.055))

    // MARK: - App Sidebar
    /// The project sidebar is a flush column beside the detail, not a floating panel.
    static let sidebarBackground = darkOnly((0.12, 0.12, 0.13), light: .windowBackgroundColor)

    // MARK: - Cards
    static let cardBackground = adaptive(dark: (0.14, 0.14, 0.15, 1.0), light: (1.0, 1.0, 1.0, 1.0))
    static let cardHeaderBackground = adaptive(dark: (0.16, 0.16, 0.17, 1.0), light: (0.93, 0.925, 0.92, 1.0))

    // MARK: - Diff
    static let additionBackground = adaptive(dark: (0.13, 0.22, 0.15, 1.0), light: (0.82, 0.93, 0.84, 1.0))
    static let deletionBackground = adaptive(dark: (0.25, 0.13, 0.13, 1.0), light: (0.94, 0.82, 0.82, 1.0))
    static let additionText = adaptive(dark: (0.55, 0.85, 0.55, 1.0), light: (0.10, 0.42, 0.10, 1.0))
    static let deletionText = adaptive(dark: (0.90, 0.55, 0.55, 1.0), light: (0.58, 0.10, 0.10, 1.0))
    static let hunkSeparator = adaptiveGray(dark: 0.22, light: 0.82)

    // MARK: - Calendar
    static let calendarGridLine = adaptiveGray(dark: 0.22, light: 0.85)
    static let calendarTodayBackground = adaptive(dark: (0.15, 0.20, 0.35, 0.3), light: (0.90, 0.93, 1.0, 0.5))

    // MARK: - Helpers

    private static func adaptive(
        dark: (CGFloat, CGFloat, CGFloat, CGFloat), light: (CGFloat, CGFloat, CGFloat, CGFloat)
    ) -> NSColor {
        #if os(macOS)
        NSColor(name: nil) { appearance in
            if appearance.bestMatch(from: [.darkAqua, .aqua]) == .darkAqua {
                return NSColor(srgbRed: dark.0, green: dark.1, blue: dark.2, alpha: dark.3)
            } else {
                return NSColor(srgbRed: light.0, green: light.1, blue: light.2, alpha: light.3)
            }
        }
        #else
        NSColor { traits in
            if traits.userInterfaceStyle == .dark {
                return NSColor(srgbRed: dark.0, green: dark.1, blue: dark.2, alpha: dark.3)
            } else {
                return NSColor(srgbRed: light.0, green: light.1, blue: light.2, alpha: light.3)
            }
        }
        #endif
    }

    private static func darkOnly(_ dark: (CGFloat, CGFloat, CGFloat), alpha: CGFloat = 1.0, light: NSColor) -> NSColor {
        #if os(macOS)
        NSColor(name: nil) { appearance in
            if appearance.bestMatch(from: [.darkAqua, .aqua]) == .darkAqua {
                return NSColor(srgbRed: dark.0, green: dark.1, blue: dark.2, alpha: alpha)
            } else {
                return light
            }
        }
        #else
        NSColor { traits in
            if traits.userInterfaceStyle == .dark {
                return NSColor(srgbRed: dark.0, green: dark.1, blue: dark.2, alpha: alpha)
            } else {
                return light
            }
        }
        #endif
    }

    private static func adaptiveGray(dark: CGFloat, light: CGFloat) -> NSColor {
        #if os(macOS)
        NSColor(name: nil) { appearance in
            if appearance.bestMatch(from: [.darkAqua, .aqua]) == .darkAqua {
                return NSColor(white: dark, alpha: 1.0)
            } else {
                return NSColor(white: light, alpha: 1.0)
            }
        }
        #else
        NSColor { traits in
            if traits.userInterfaceStyle == .dark {
                return NSColor(white: dark, alpha: 1.0)
            } else {
                return NSColor(white: light, alpha: 1.0)
            }
        }
        #endif
    }
}
