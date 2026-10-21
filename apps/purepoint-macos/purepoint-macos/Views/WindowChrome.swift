#if os(macOS)
import AppKit
import OSLog
import SwiftUI

private let logger = Logger(subsystem: "com.purepoint.macos", category: "WindowChrome")

/// Makes the window's content view span the title bar, so the sidebar and each pane's tab strip
/// start at the top edge. `.ignoresSafeArea` only removes a safe-area inset; it cannot move content
/// that AppKit lays out below the title bar, which happens unless the window has `.fullSizeContentView`.
struct WindowChromeConfigurator: NSViewRepresentable {
    func makeNSView(context: Context) -> NSView { WindowChromeView() }

    func updateNSView(_ nsView: NSView, context: Context) {
        (nsView as? WindowChromeView)?.configureWindow()
    }
}

private final class WindowChromeView: NSView {
    override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        configureWindow()
        guard let window else { return }
        // If the strip still stops short, these say whether AppKit or SwiftUI holds the gap.
        logger.debug(
            "fullSizeContentView=\(window.styleMask.contains(.fullSizeContentView)) toolbar=\(window.toolbar != nil) contentTopInset=\(Double(window.contentView?.safeAreaInsets.top ?? -1))"
        )
    }

    /// Reapplied on updates in case SwiftUI resets the window's style.
    func configureWindow() {
        guard let window else { return }
        window.styleMask.insert(.fullSizeContentView)
        window.titlebarAppearsTransparent = true
        window.titleVisibility = .hidden
        window.titlebarSeparatorStyle = .none
    }

    // Purely a window hook; never take clicks from the content above it.
    override func hitTest(_ point: NSPoint) -> NSView? { nil }
}
#endif
