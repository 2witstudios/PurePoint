import SwiftUI

extension View {
    /// The app root's control defaults for a sheet or popover's content. On iOS a presentation
    /// does not inherit the root's default font and styles (`MacControlStyles+iOS.swift`), so
    /// each presented view sets them again. On the Mac the system defaults already are these, and
    /// this returns the view unchanged.
    @ViewBuilder
    func presentedControlDefaults() -> some View {
        #if os(iOS)
        // Sized to its content, as a Mac sheet window is.
        font(.body).macControlDefaults().presentationSizing(.fitted)
        #else
        self
        #endif
    }
}
