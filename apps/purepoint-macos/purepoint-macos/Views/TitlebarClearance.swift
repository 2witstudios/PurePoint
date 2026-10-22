import SwiftUI

/// What the window's top strip must clear while the left sidebar is hidden and the detail starts at
/// the window's leading edge: the traffic lights and the sidebar toggle. Only the top strip moves
/// aside; the content below it runs to the edge.
struct TitlebarClearance: Equatable {
    /// How far the strip's contents start from the leading edge.
    var leading: CGFloat
    /// The title bar's height, for views that have no strip of their own and start below it instead.
    var height: CGFloat

    static let none = TitlebarClearance(leading: 0, height: 0)
}

private struct TitlebarClearanceKey: EnvironmentKey {
    static let defaultValue = TitlebarClearance.none
}

extension EnvironmentValues {
    /// Set by the window while its left sidebar is hidden; `.none` otherwise.
    var titlebarClearance: TitlebarClearance {
        get { self[TitlebarClearanceKey.self] }
        set { self[TitlebarClearanceKey.self] = newValue }
    }
}
