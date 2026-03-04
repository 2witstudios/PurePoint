import SwiftUI

enum PurePointTheme {
    // MARK: - Spacing
    static let navRowHeight: CGFloat = 26
    static let sidebarRowHeight: CGFloat = 24
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

    // MARK: - Settings
    static let settingsWidth: CGFloat = 680
    static let settingsHeight: CGFloat = 480
    static let settingsSidebarWidth: CGFloat = 180

    // MARK: - Calendar
    static let calendarHourHeight: CGFloat = 60
    static let calendarTimeGutterWidth: CGFloat = 56
    static let calendarHeaderHeight: CGFloat = 40
    static let calendarPillHeight: CGFloat = 16

    // MARK: - Fonts
    static let navFont = Font.system(size: 12, weight: .medium)
    static let treeFont = Font.system(size: 12)
    static let smallFont = Font.system(size: 11)
}
