import SwiftUI
import OSLog

// Isolated simulator verification app, excluded from both production targets.
// It hosts the real phone chat against a local demo snapshot, without a bridge.
enum LaunchLog { static let logger = Logger(subsystem: "PointGuardMarkdownPreview", category: "Launch") }
@main struct PhonePreview: App {
    @StateObject private var model: ChatModel
    @State private var dark = ProcessInfo.processInfo.arguments.contains("--dark")
    @State private var large = ProcessInfo.processInfo.arguments.contains("--large-type")
    init() {
        let model = ChatModel()
        model.exploreDemo()
        model.snapshot = Self.snapshot(code: ProcessInfo.processInfo.arguments.contains("--code"), revision: 2)
        _model = StateObject(wrappedValue: model)
    }
    private static func snapshot(code: Bool, revision: Int, text: String? = nil) -> Snapshot {
        let reply = code ? """
        ### Ready to run
        1. Check the workspace:
           ```sh
           pu status --global --json
           pu agents list --global --json
           ```

        | Platform | Rendering | Status |
        | :--- | :--- | ---: |
        | Desktop | Native Markdown | Ready |
        | iOS | Shared styles | Ready |
        """ : """
        ## A clearer view of the work
        Here's the **release plan**, with `pu status` ready to run.
        This line stays on its own line.

        ### Next steps
        1. Review the desktop changes
           - [x] Preserve line breaks
           - [ ] Check the phone layout
        2. Ship the polished chat

        > **Keep the conversation readable.**
        > Long replies should have a clear rhythm.
        """
        return Snapshot(version: 1, epoch: "preview", revision: revision, busy: false, sessionId: "preview", title: "Markdown preview", messages: [
            ChatMessage(id: "user", role: "user", text: "Show me the release plan."),
            ChatMessage(id: "assistant", role: "assistant", text: text ?? reply)
        ], tools: [], queue: [], dialogs: [], notices: [])
    }
    var body: some Scene {
        WindowGroup {
            ChatView(model: model)
                .safeAreaInset(edge: .bottom) {
                    Menu("Preview controls") {
                        Button("Overview") { model.snapshot = Self.snapshot(code: false, revision: (model.snapshot?.revision ?? 0) + 1) }
                        Button("Code and table") { model.snapshot = Self.snapshot(code: true, revision: (model.snapshot?.revision ?? 0) + 1) }
                        Button("Long reply") {
                            let text = (1...12).map { "### Step \($0)\nRead the result, then check the next task.\n\n```sh\npu status --global --json\n```" }.joined(separator: "\n\n") + "\n\nEnd of the long reply."
                            model.snapshot = Self.snapshot(code: false, revision: (model.snapshot?.revision ?? 0) + 1, text: text)
                        }
                        Button("Append a section") {
                            let text = (model.snapshot?.messages.last?.text ?? "") + "\n\n## New section\nThis arrived after the reply was laid out.\n\n```swift\nlet next = \"Keep following the reply\"\n```\n\nEnd of the appended section."
                            model.snapshot = Self.snapshot(code: false, revision: (model.snapshot?.revision ?? 0) + 1, text: text)
                        }
                        Toggle("Dark appearance", isOn: $dark)
                        Toggle("Large type", isOn: $large)
                    }.font(.caption).frame(maxWidth: .infinity).padding(4)
                }
                .tint(.accentColor)
                .environment(\.colorScheme, dark ? .dark : .light)
                .environment(\.dynamicTypeSize, large ? .accessibility2 : .large)
        }
    }
}
