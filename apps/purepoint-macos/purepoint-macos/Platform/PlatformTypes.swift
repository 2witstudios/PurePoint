import SwiftTerm

#if os(macOS)
import AppKit
#else
import UIKit
#endif

/// The SwiftTerm terminal view. SwiftTerm names it `TerminalView` on both platforms (an
/// `NSView` on macOS, a `UIView` on iOS), so shared code refers to it through this alias and
/// keeps the platform-specific view APIs here.
typealias PlatformTerminalView = SwiftTerm.TerminalView

extension PlatformTerminalView {
    /// Whether the terminal currently receives keyboard input.
    var hasKeyboardFocus: Bool {
        #if os(macOS)
        window?.firstResponder === self
        #else
        isFirstResponder
        #endif
    }

    /// Hand the daemon's reported PTY size to the `ScrollableTerminal` hosting this view.
    func applyReportedPtySize(cols: Int, rows: Int) {
        (superview as? ScrollableTerminal)?.applyReportedSize(cols: cols, rows: rows)
    }
}
