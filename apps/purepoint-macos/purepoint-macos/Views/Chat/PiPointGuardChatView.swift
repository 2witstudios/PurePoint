import SwiftUI

/// Native presentation of the same authoritative Pi session used by the phone.
struct PiPointGuardChatView: View {
    @ObservedObject var model: PiChatModel
    @Binding var showSidebar: Bool
    @State private var search = ""
    @State private var pendingSession: PiConversation?
    @State private var confirmNew = false
    @State private var confirmResume = false
    @State private var followOutput = true
    @FocusState private var composerFocused: Bool
    @Environment(AppState.self) private var appState

    var body: some View {
        HSplitView {
            if showSidebar { sidebar.frame(minWidth: 180, idealWidth: 210, maxWidth: 280) }
            VStack(spacing: 0) {
                header
                Divider()
                transcript
                if let error = model.error {
                    InlineErrorBanner(message: error) { model.error = nil }
                }
                if model.browsing == nil { composer }
            }
            .frame(minWidth: 320, maxWidth: .infinity, maxHeight: .infinity)
        }
        .task { if model.connected { await model.loadConversations() } }
        .onChange(of: model.connected) { _, connected in
            if connected { Task { await model.loadConversations() } }
        }
        .onChange(of: model.busy) { _, busy in
            if !busy && model.connected { Task { await model.loadConversations() } }
        }
        .onChange(of: model.snapshot?.sessionId) { _, _ in
            Task { await model.loadConversations() }
        }
        .confirmationDialog("Stop Pi before starting a new conversation?", isPresented: $confirmNew) {
            Button("Stop and start new") { Task { await model.changeSession(stopFirst: true) } }
        }
        .confirmationDialog("Stop Pi before resuming this conversation?", isPresented: $confirmResume) {
            Button("Stop and resume") {
                if let pendingSession { Task { await model.changeSession(to: pendingSession.id, stopFirst: true) } }
            }
        }
        .sheet(
            item: Binding(
                get: { model.snapshot?.dialogs.first },
                set: { dialog in
                    if dialog == nil, let current = model.snapshot?.dialogs.first {
                        Task { await model.answer(current, cancelled: true) }
                    }
                })
        ) { dialog in PiDialogView(model: model, dialog: dialog).id(dialog.id) }
    }

    private var header: some View {
        HStack(spacing: 10) {
            if !showSidebar {
                Button {
                    showSidebar = true
                } label: {
                    Image(systemName: "sidebar.left")
                }
                .help("Show conversations")
            }
            VStack(alignment: .leading, spacing: 3) {
                Text(model.snapshot?.title == "Pi" ? "Point Guard" : model.snapshot?.title ?? "Point Guard")
                    .font(.system(size: 14, weight: .semibold)).lineLimit(1)
                Text(model.connectionStatus).font(.system(size: 11)).foregroundStyle(.secondary)
                if let root = appState.activeProjectRoot {
                    Text("Workspace: " + URL(fileURLWithPath: root).lastPathComponent)
                        .font(.system(size: 11)).foregroundStyle(.secondary)
                        .help("Pi is a global assistant. Name the project when directing work.")
                }
            }
            Spacer()
            if model.busy {
                ProgressView().controlSize(.small)
                Text("Working").font(.system(size: 11)).foregroundStyle(.secondary)
            }
            Button {
                if model.connected { model.disconnect() } else { model.connect() }
            } label: {
                Image(systemName: model.connected ? "bolt.slash" : "bolt")
            }
            .help(model.connected ? "Disconnect this view; Pi and other clients keep running" : "Connect to Pi")
        }
        .buttonStyle(.borderless)
        .padding(12)
    }

    private var sidebar: some View {
        VStack(spacing: 0) {
            HStack {
                Text("Conversations").font(.system(size: 12, weight: .semibold))
                Spacer()
                Button {
                    showSidebar = false
                } label: {
                    Image(systemName: "sidebar.left")
                }
                .help("Hide conversations")
                Button {
                    if model.busy { confirmNew = true } else { Task { await model.changeSession() } }
                } label: {
                    Image(systemName: "square.and.pencil")
                }
                .help("New conversation").disabled(!model.connected || model.changingSession)
            }.buttonStyle(.borderless).padding(12)
            TextField("Search conversations", text: $search).textFieldStyle(.roundedBorder)
                .padding(.horizontal, 10).padding(.bottom, 10)
            List(model.conversations.filter { search.isEmpty || $0.title.localizedCaseInsensitiveContains(search) }) {
                session in
                Button {
                    if model.busy {
                        pendingSession = session
                        Task { await model.browse(session) }
                    } else {
                        Task { await model.changeSession(to: session.id) }
                    }
                } label: {
                    HStack {
                        Text(session.title).lineLimit(2).multilineTextAlignment(.leading)
                        Spacer(minLength: 0)
                        if model.snapshot?.sessionId == session.id {
                            Image(systemName: "checkmark").foregroundStyle(Color.accentColor)
                        }
                    }.font(.system(size: 12)).padding(.vertical, 3)
                }.buttonStyle(.plain).disabled(!model.connected || model.changingSession)
            }.listStyle(.sidebar)
            HStack {
                Button("Refresh", systemImage: "arrow.clockwise") { Task { await model.loadConversations() } }
                    .disabled(!model.connected)
                Spacer()
                Button {
                    appState.showSettings = true
                } label: {
                    Image(systemName: "gearshape")
                }.help("Point Guard connection settings")
            }.font(.system(size: 11)).buttonStyle(.borderless).padding(10)
        }
    }

    private var transcript: some View {
        ScrollViewReader { proxy in
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 20) {
                    if let history = model.browsing {
                        HStack {
                            Text("Reading: " + history.title).font(.system(size: 12, weight: .medium))
                            Spacer()
                            Button("Resume") {
                                pendingSession = PiConversation(id: history.sessionId, title: history.title);
                                if model.busy {
                                    confirmResume = true
                                } else {
                                    Task { await model.changeSession(to: history.sessionId) }
                                }
                            }
                            Button("Back to live") { model.browsing = nil }
                        }
                        ForEach(history.messages) { message in messageView(message) }
                    } else if model.transcriptRows.isEmpty {
                        emptyState.padding(.vertical, 70)
                    } else {
                        ForEach(model.transcriptRows) { row in
                            switch row {
                            case .message(let message): messageView(message)
                            case .activity(let tools): PiToolActivityView(tools: tools)
                            }
                        }
                    }
                    if model.browsing == nil {
                        ForEach(Array((model.snapshot?.notices ?? []).enumerated()), id: \.offset) { _, notice in
                            Text(notice).font(.system(size: 12)).foregroundStyle(.secondary).textSelection(.enabled)
                        }
                    }
                    Color.clear.frame(height: 1).id("bottom")
                }
                .frame(maxWidth: 760, alignment: .leading)
                .frame(maxWidth: .infinity)
                .padding(24)
            }
            .onScrollPhaseChange { oldPhase, phase, context in
                if phase == .tracking || phase == .interacting { followOutput = false }
                if phase == .idle && oldPhase != .animating {
                    let geometry = context.geometry
                    followOutput =
                        geometry.contentOffset.y + geometry.containerSize.height >= geometry.contentSize.height - 96
                }
            }
            .onChange(of: model.snapshot?.sessionId) { _, _ in
                followOutput = true
                proxy.scrollTo("bottom", anchor: .bottom)
            }
            .onChange(of: model.snapshot?.revision) { _, _ in
                if followOutput && model.browsing == nil { proxy.scrollTo("bottom", anchor: .bottom) }
            }
            .overlay(alignment: .bottomTrailing) {
                if !followOutput {
                    Button("Latest", systemImage: "arrow.down") {
                        followOutput = true; proxy.scrollTo("bottom", anchor: .bottom)
                    }
                    .buttonStyle(.bordered).padding(12)
                }
            }
        }
    }

    private var emptyState: some View {
        VStack(alignment: .leading, spacing: 16) {
            Image(systemName: "bubble.left.and.bubble.right").font(.system(size: 30)).foregroundStyle(Color.accentColor)
            Text("What would you like to work on?").font(.system(size: 24, weight: .semibold))
            Text(
                model.connected
                    ? "Plan a change, direct your agents, or explore an idea with Point Guard."
                    : "Connect to your Mac’s Pi bridge in Settings → Point Guard to start a conversation."
            )
            .font(.system(size: 14)).foregroundStyle(.secondary)
            if !model.connected {
                Button("Open settings") { appState.showSettings = true }.buttonStyle(.bordered)
            }
        }.frame(maxWidth: .infinity, alignment: .leading)
    }

    private func messageView(_ message: PiChatMessage) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            Text(message.role == "user" ? "You" : message.role == "assistant" ? "Point Guard" : message.role)
                .font(PurePointTheme.chatLabelFont).foregroundStyle(.secondary)
            if message.role == "user" {
                Text(message.text).font(.system(size: 14)).textSelection(.enabled)
            } else {
                ForEach(Array(PiMarkdownBlocks.split(message.text).enumerated()), id: \.offset) { _, block in
                    switch block {
                    case .prose(let text): MarkdownTextView(text: text)
                    case .code(let language, let text):
                        CodeBlockView(language: language.isEmpty ? nil : language, code: text)
                    }
                }
            }
            if let error = message.error { Text(error).foregroundStyle(.red).textSelection(.enabled) }
        }.frame(maxWidth: .infinity, alignment: .leading)
    }

    private var composer: some View {
        VStack(alignment: .leading, spacing: 8) {
            ForEach(model.recoverable) { receipt in
                HStack {
                    VStack(alignment: .leading, spacing: 3) {
                        Text(receipt.status).font(.system(size: 11)).lineLimit(2)
                        Text(receipt.text).font(.system(size: 11)).foregroundStyle(.secondary).lineLimit(1)
                    }
                    Spacer()
                    Button("Restore") { model.restore(receipt) }
                    Button("Dismiss") { model.dismissSubmission(receipt.id) }
                }
            }
            if model.submissions.contains(where: { $0.status == "Sending" }) {
                Text("Sending…").font(.system(size: 11)).foregroundStyle(.secondary)
            }
            if let queue = model.snapshot?.queue, !queue.isEmpty {
                Text("\(queue.count) queued \(queue.count == 1 ? "message" : "messages")")
                    .font(.system(size: 11)).foregroundStyle(.secondary)
            }
            if model.editorOffer != nil { Button("Add suggested text to draft") { model.useEditorOffer() } }
            HStack(alignment: .bottom, spacing: 12) {
                TextField("Message Point Guard…", text: $model.draft, axis: .vertical)
                    .textFieldStyle(.plain).font(.system(size: 14)).lineLimit(1...10).focused($composerFocused)
                    .onKeyPress(keys: [.return], phases: .down) { press in
                        if press.modifiers.contains(.shift) { return .ignored }
                        if model.canSend { model.submit(mode: model.busy ? "after" : "send") }
                        return .handled
                    }
                if model.busy {
                    Button {
                        Task { _ = await model.stop() }
                    } label: {
                        Image(systemName: "stop.fill")
                    }
                    .help("Stop Pi and queued messages; delegated workers keep running").disabled(!model.connected)
                    Menu {
                        Button("Send after reply") { model.submit(mode: "after") }
                        Button("Steer current work") { model.submit(mode: "steer") }
                    } label: {
                        Image(systemName: "arrow.up.circle.fill").font(.system(size: 24))
                    }
                    .disabled(!model.canSend).help("Send options")
                } else {
                    Button {
                        model.submit(mode: "send")
                    } label: {
                        Image(systemName: "arrow.up.circle.fill").font(.system(size: 24))
                    }
                    .buttonStyle(.plain).disabled(!model.canSend).help("Send message (Return)")
                }
            }
            .padding(14)
            .background(Color(NSColor.controlBackgroundColor), in: RoundedRectangle(cornerRadius: 12))
            .overlay(RoundedRectangle(cornerRadius: 12).strokeBorder(Color.primary.opacity(0.1)))
            Text("Return to send · Shift-Return for a new line").font(.system(size: 10)).foregroundStyle(.tertiary)
        }
        .frame(maxWidth: 760).frame(maxWidth: .infinity).padding(.horizontal, 24).padding(.bottom, 16)
        .onAppear { composerFocused = true }
    }
}

private struct PiToolActivityView: View {
    let tools: [PiToolActivity]
    var body: some View {
        DisclosureGroup {
            ForEach(tools) { tool in
                DisclosureGroup {
                    ScrollView(.horizontal) {
                        Text(tool.text).font(.system(size: 12, design: .monospaced)).textSelection(.enabled).padding(
                            .vertical, 6)
                    }
                } label: {
                    Label(
                        tool.name + " · " + tool.state,
                        systemImage: tool.state == "running"
                            ? "gearshape" : tool.state == "finished" ? "checkmark.circle" : "exclamationmark.circle"
                    )
                    .font(.system(size: 12))
                }.padding(.vertical, 4)
            }
        } label: {
            Label(
                "\(tools.count) \(tools.count == 1 ? "tool" : "tools")"
                    + (tools.contains { $0.state == "running" } ? " working" : " used"),
                systemImage: "wrench.and.screwdriver"
            )
            .font(.system(size: 12)).foregroundStyle(.secondary)
        }.padding(12).background(Color.primary.opacity(0.035), in: RoundedRectangle(cornerRadius: 8))
    }
}

private struct PiDialogView: View {
    @ObservedObject var model: PiChatModel
    let dialog: PiExtensionDialog
    @State private var text = ""
    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text(dialog.title ?? "Point Guard needs your input").font(.headline)
            if let message = dialog.message { Text(message).textSelection(.enabled) }
            if dialog.method == "select" {
                ForEach(Array((dialog.options ?? []).enumerated()), id: \.offset) { index, option in
                    Button(option) {
                        Task {
                            await model.answer(
                                dialog,
                                optionId: dialog.optionIds.flatMap { $0.indices.contains(index) ? $0[index] : nil })
                        }
                    }
                }
            } else if dialog.method != "confirm" {
                TextEditor(text: $text).frame(minHeight: 100).border(Color.secondary.opacity(0.2))
                    .accessibilityLabel(dialog.placeholder ?? "Response")
            }
            HStack {
                Button("Cancel") { Task { await model.answer(dialog, cancelled: true) } }.keyboardShortcut(
                    .cancelAction)
                Spacer()
                if dialog.method == "confirm" {
                    Button("No") { Task { await model.answer(dialog, confirmed: false) } }
                    Button("Confirm") { Task { await model.answer(dialog, confirmed: true) } }.keyboardShortcut(
                        .defaultAction)
                } else if dialog.method != "select" {
                    Button("Send") { Task { await model.answer(dialog, value: text) } }.keyboardShortcut(.defaultAction)
                }
            }
        }.padding(24).frame(width: 420).onAppear { text = dialog.prefill ?? "" }
    }
}
