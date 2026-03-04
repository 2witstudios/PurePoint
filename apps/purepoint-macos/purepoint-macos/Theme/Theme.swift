import AppKit

nonisolated enum Theme {
    // MARK: - Text
    static let primaryText = adaptive(dark: (0.85, 0.85, 0.87, 1.0), light: (0.10, 0.10, 0.12, 1.0))

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

    private static func adaptive(dark: (CGFloat, CGFloat, CGFloat, CGFloat), light: (CGFloat, CGFloat, CGFloat, CGFloat)) -> NSColor {
        NSColor(name: nil) { appearance in
            if appearance.bestMatch(from: [.darkAqua, .aqua]) == .darkAqua {
                return NSColor(srgbRed: dark.0, green: dark.1, blue: dark.2, alpha: dark.3)
            } else {
                return NSColor(srgbRed: light.0, green: light.1, blue: light.2, alpha: light.3)
            }
        }
    }

    private static func adaptiveGray(dark: CGFloat, light: CGFloat) -> NSColor {
        NSColor(name: nil) { appearance in
            if appearance.bestMatch(from: [.darkAqua, .aqua]) == .darkAqua {
                return NSColor(white: dark, alpha: 1.0)
            } else {
                return NSColor(white: light, alpha: 1.0)
            }
        }
    }
}
