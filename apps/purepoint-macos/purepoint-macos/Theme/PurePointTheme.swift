import SwiftUI

enum PurePointTheme {
    // MARK: - Spacing
    static let navRowHeight: CGFloat = 26
    static let statusDotSize: CGFloat = 6
    static let padding: CGFloat = 8
    static let footerHeight: CGFloat = 36
    static let treeIndent: CGFloat = 16

    // MARK: - Sidebar Width
    static let sidebarMinWidth: CGFloat = 200
    static let sidebarIdealWidth: CGFloat = 240
    static let sidebarMaxWidth: CGFloat = 320

    // MARK: - Window
    static let windowMinWidth: CGFloat = 700
    static let windowMinHeight: CGFloat = 400
    static let windowDefaultWidth: CGFloat = 1000
    static let windowDefaultHeight: CGFloat = 700

    // MARK: - Colors
    static let contentBackground = Color(red: 0.11, green: 0.11, blue: 0.12)
    static let selectionHighlight = Color.white.opacity(0.08)
    static let hoverHighlight = Color.white.opacity(0.05)
    static let primaryText = Color(red: 0.85, green: 0.85, blue: 0.87)
    static let secondaryText = Color(red: 0.55, green: 0.55, blue: 0.57)
    static let tertiaryText = Color(red: 0.40, green: 0.40, blue: 0.42)
    static let badgeBackground = Color.white.opacity(0.12)

    // MARK: - Fonts
    static let navFont = Font.system(size: 12, weight: .medium)
    static let treeFont = Font.system(size: 12)
    static let smallFont = Font.system(size: 11)
}
