import AppKit

/// Syntax highlight colors derived from TerminalTheme ANSI palette for visual consistency.
nonisolated enum EditorTheme {
    // MARK: - Syntax Colors

    static let keyword = adaptive(dark: (0.72, 0.53, 0.86), light: (0.48, 0.25, 0.64))
    static let string = adaptive(dark: (0.43, 0.68, 0.47), light: (0.18, 0.49, 0.20))
    static let comment = adaptive(dark: (0.50, 0.50, 0.52), light: (0.50, 0.50, 0.52))
    static let number = adaptive(dark: (0.79, 0.67, 0.40), light: (0.55, 0.43, 0.12))
    static let type = adaptive(dark: (0.39, 0.72, 0.78), light: (0.12, 0.44, 0.52))
    static let function = adaptive(dark: (0.43, 0.61, 0.90), light: (0.18, 0.37, 0.69))
    static let property = adaptive(dark: (0.78, 0.35, 0.35), light: (0.71, 0.23, 0.23))
    static let preprocessor = adaptive(dark: (0.90, 0.78, 0.50), light: (0.64, 0.52, 0.18))

    // MARK: - Editor Colors

    static let background = Theme.cardBackground
    static let lineNumberColor = NSColor.tertiaryLabelColor
    static let currentLineHighlight = adaptive(dark: (0.16, 0.16, 0.17), light: (0.96, 0.96, 0.96))

    // MARK: - Helpers

    private static func adaptive(
        dark: (CGFloat, CGFloat, CGFloat), light: (CGFloat, CGFloat, CGFloat)
    ) -> NSColor {
        NSColor(name: nil) { appearance in
            if appearance.bestMatch(from: [.darkAqua, .aqua]) == .darkAqua {
                return NSColor(srgbRed: dark.0, green: dark.1, blue: dark.2, alpha: 1.0)
            } else {
                return NSColor(srgbRed: light.0, green: light.1, blue: light.2, alpha: 1.0)
            }
        }
    }
}
