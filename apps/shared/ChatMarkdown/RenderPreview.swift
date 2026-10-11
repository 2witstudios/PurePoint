import AppKit
import SwiftUI

// Standalone visual fixture; excluded from both application targets.
@main struct Preview {
    @MainActor static func main() {
        let app = NSApplication.shared
        app.setActivationPolicy(.prohibited)
        let source = """
        ## A clearer view of the work
        Here's the **release plan**, with `pu status` ready to run.
        This line stays on its own line.

        ### Next steps
        1. Review the desktop changes
           - [x] Preserve line breaks
           - [ ] Check the phone layout
        2. Ship the polished chat

        > **Keep the conversation readable.**
        > Long replies should have a clear rhythm, even in narrow panes.

        | Platform | Rendering | Status |
        | :--- | :--- | ---: |
        | Desktop | Native Markdown | Ready |
        | iOS | Shared styles | Ready |

        ```swift
        let conversation = PointGuard()
        conversation.send("Hello, Markdown")
        ```

        Read the [project notes](https://example.com) for details.
        """
        var windows: [NSWindow] = []
        for (name, width, dark) in [("desktop-light", 760.0, false), ("desktop-dark", 760.0, true), ("phone-width", 390.0, false)] {
            let content = ChatMarkdownView(text: source).padding(24).frame(width: width).background(dark ? Color.black : Color.white).environment(\.colorScheme, dark ? .dark : .light)
            let host = NSHostingView(rootView: content)
            let window = NSWindow(contentRect: NSRect(x: -10000, y: -10000, width: width, height: 1000), styleMask: [.borderless], backing: .buffered, defer: false)
            window.contentView = host; window.orderBack(nil); windows.append(window)
            Timer.scheduledTimer(withTimeInterval: 1.0, repeats: false) { _ in
                MainActor.assumeIsolated {
                    host.layoutSubtreeIfNeeded()
                    guard let bitmap = host.bitmapImageRepForCachingDisplay(in: host.bounds) else { fatalError("No bitmap") }
                    host.cacheDisplay(in: host.bounds, to: bitmap)
                    try! bitmap.representation(using: .png, properties: [:])!.write(to: URL(fileURLWithPath: "/tmp/pointguard-\(name).png"))
                    if name == "phone-width" { exit(0) }
                }
            }
        }
        app.run()
    }
}
