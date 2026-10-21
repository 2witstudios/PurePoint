import AppKit
import SwiftUI
import Observation

// Minimal app chrome for an isolated host of the actual desktop chat.
// Excluded from production targets; never starts Pi, Cargo or the daemon.
@MainActor @Observable final class AppState {
    var showSettings = false
    func pointGuardPrompts() async -> [PointGuardPrompt] { [] }
    func pointGuardPromptBody(_ name: String) async -> String? { nil }
}
struct InlineErrorBanner: View {
    let message: String
    let dismiss: () -> Void
    var body: some View { HStack { Text(message); Button("Dismiss", action: dismiss) }.padding() }
}
@main struct DesktopChatPreview {
    @MainActor static func main() {
        let application = NSApplication.shared
        application.setActivationPolicy(.prohibited)
        let defaults = UserDefaults(suiteName: "PointGuardMarkdownPreview")!
        let model = PiChatModel(defaults: defaults)
        model.exploreDemo()
        model.snapshot = PiSnapshot(version: 1, epoch: "preview", revision: 2, busy: false, sessionId: "preview", title: "Markdown preview", messages: [
            PiChatMessage(id: "user", role: "user", text: "Show me the release plan."),
            PiChatMessage(id: "assistant", role: "assistant", text: """
            ## A clearer view of the work
            Here's the **release plan**, with `pu status` ready to run.
            This line stays on its own line.

            1. Check the workspace:
               ```sh
               pu status --global --json
               pu agents list --global --json
               ```
            2. Review the results
               - [x] Preserve line breaks
               - [ ] Check the phone layout

            > **Keep the conversation readable.**
            > Long replies should have a clear rhythm.

            | Platform | Rendering | Status |
            | :--- | :--- | ---: |
            | Desktop | Native Markdown | Ready |
            | iOS | Shared styles | Ready |
            """)
        ], tools: [], queue: [], dialogs: [], notices: [])
        // A fresh conversation opens on the Point Guard court.
        let court = PiChatModel(defaults: defaults)
        court.exploreDemo()
        court.snapshot = PiSnapshot(version: 1, epoch: "preview", revision: 1, busy: false, sessionId: "court", title: "New conversation", messages: [], tools: [], queue: [], dialogs: [], notices: [])
        var windows: [NSWindow] = []
        let renders: [(String, Double, Bool, PiChatModel)] = [
            ("desktop-chat-light", 940, false, model), ("desktop-chat-dark", 940, true, model),
            ("desktop-court-dark", 1280, true, court), ("desktop-court-light", 1280, false, court),
            ("desktop-court-squeezed", 900, true, court), ("desktop-court-narrow", 400, true, court),
            ("desktop-chat-narrow", 400, false, model),
        ]
        for (name, width, dark, model) in renders {
            let view = PiPointGuardChatView(model: model, showSidebar: .constant(false))
                .environment(AppState()).environment(\.colorScheme, dark ? .dark : .light)
                .frame(width: width, height: 950).background(dark ? Color.black : Color.white)
            let host = NSHostingView(rootView: view)
            let window = NSWindow(contentRect: NSRect(x: -10000, y: -10000, width: width, height: 950), styleMask: [.borderless], backing: .buffered, defer: false)
            window.contentView = host; window.orderBack(nil); windows.append(window)
            Timer.scheduledTimer(withTimeInterval: 1, repeats: false) { _ in
                MainActor.assumeIsolated {
                    host.layoutSubtreeIfNeeded()
                    guard let bitmap = host.bitmapImageRepForCachingDisplay(in: host.bounds) else { fatalError("No bitmap") }
                    host.cacheDisplay(in: host.bounds, to: bitmap)
                    try! bitmap.representation(using: .png, properties: [:])!.write(to: URL(fileURLWithPath: "/tmp/pointguard-\(name).png"))
                    if name == "desktop-chat-narrow" { exit(0) }
                }
            }
        }
        application.run()
    }
}
