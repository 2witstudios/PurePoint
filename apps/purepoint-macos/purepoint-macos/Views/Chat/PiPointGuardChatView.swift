import SwiftUI
import UniformTypeIdentifiers

/// Native presentation of the same authoritative Pi session used by the phone.
struct PiPointGuardChatView: View {
    @ObservedObject var model: PiChatModel
    @Binding var showSidebar: Bool
    var isFocused = true
    var compact = false
    @State private var search = ""
    @State private var pendingSession: PiConversation?
    @State private var confirmNew = false
    @State private var confirmResume = false
    @State private var followOutput = true
    @State private var scrollingTranscript = false
    @State private var showFiles = false
    @State private var importingAttachment = false
    @FocusState private var composerFocused: Bool
    @Environment(AppState.self) private var appState

    var body: some View {
        PointGuardConversationLayout(isExpanded: $showSidebar, compact: compact) {
            Button {
                if model.busy { confirmNew = true } else { Task { await model.changeSession() } }
            } label: {
                Label("New Pi Chat", systemImage: "square.and.pencil")
            }
            .help("Start a new Pi conversation")
            .accessibilityLabel("New Pi Chat")
            .disabled(!model.connected || model.changingSession)
        } sidebar: {
            sidebar
        } content: {
            VStack(spacing: 0) {
                transcript
                if let error = model.error {
                    InlineErrorBanner(message: error) { model.error = nil }
                }
                if model.browsing == nil { composer }
            }
            .frame(minWidth: 0, maxWidth: .infinity, maxHeight: .infinity)
        }
        .fileImporter(isPresented: $showFiles, allowedContentTypes: [.image, .plainText, .sourceCode, .json, .pdf]) {
            result in
            importAttachment(result)
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

    private var sidebar: some View {
        VStack(spacing: 0) {
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
                LazyVStack(alignment: .leading, spacing: 24) {
                    if let history = model.browsing {
                        VStack(alignment: .leading, spacing: 8) {
                            Text("Reading: " + history.title).font(.system(size: 12, weight: .medium))
                            HStack {
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
                        }
                        ForEach(PiTranscriptRows.make(messages: history.messages, tools: [])) { row in
                            switch row {
                            case .message(let message): messageView(message)
                            case .activity(let tools): PiToolActivityView(tools: tools)
                            }
                        }
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
                .frame(maxWidth: PurePointTheme.chatContentWidth, alignment: .leading)
                .frame(maxWidth: .infinity)
                .padding(.horizontal, compact ? 8 : PurePointTheme.chatHorizontalPadding)
                .padding(.vertical, 24)
            }
            .onScrollGeometryChange(for: Bool.self) { geometry in
                geometry.contentOffset.y + geometry.containerSize.height >= geometry.contentSize.height - 96
            } action: { _, atBottom in
                if atBottom && !scrollingTranscript { followOutput = true }
            }
            .onScrollGeometryChange(for: CGFloat.self) { $0.contentSize.height } action: { _, _ in
                // Markdown finishes parsing after the snapshot arrives. Follow its final layout too.
                if followOutput && !scrollingTranscript && model.browsing == nil { proxy.scrollTo("bottom", anchor: .bottom) }
            }
            .onScrollPhaseChange { oldPhase, phase, context in
                if phase == .tracking || phase == .interacting { scrollingTranscript = true; followOutput = false }
                if phase == .idle && oldPhase != .animating {
                    scrollingTranscript = false
                    let geometry = context.geometry
                    if geometry.containerSize.height > 0 {
                        followOutput = geometry.contentOffset.y + geometry.containerSize.height >= geometry.contentSize.height - 96
                    }
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

    @ViewBuilder
    private func messageView(_ message: PiChatMessage) -> some View {
        if message.role == "user" {
            HStack {
                Spacer(minLength: 48)
                VStack(alignment: .leading, spacing: 8) {
                    Text(message.text).font(PurePointTheme.chatFont).lineSpacing(3).textSelection(.enabled)
                    if let error = message.error { Text(error).foregroundStyle(.red).textSelection(.enabled) }
                }
                .padding(.horizontal, 17).padding(.vertical, 13)
                .background(Color.primary.opacity(0.055), in: RoundedRectangle(cornerRadius: 22))
                .accessibilityLabel("You: " + message.text)
            }
        } else if message.role == "notice" {
            Label(message.text, systemImage: "text.alignleft")
                .font(.system(size: 12)).foregroundStyle(.secondary).textSelection(.enabled)
        } else {
            VStack(alignment: .leading, spacing: 14) {
                ChatMarkdownView(text: message.text).equatable()
                if let error = message.error { Text(error).foregroundStyle(.red).textSelection(.enabled) }
            }.frame(maxWidth: .infinity, alignment: .leading)
        }
    }

    private var composer: some View {
        VStack(alignment: .leading, spacing: 8) {
            ForEach(model.recoverable) { receipt in
                VStack(alignment: .leading, spacing: 4) {
                    VStack(alignment: .leading, spacing: 3) {
                        Text(receipt.status).font(.system(size: 11)).lineLimit(2)
                        Text(receipt.text).font(.system(size: 11)).foregroundStyle(.secondary).lineLimit(1)
                    }
                    HStack {
                        Spacer(minLength: 0)
                        Button("Restore") { model.restore(receipt) }
                        Button("Dismiss") { model.dismissSubmission(receipt.id) }
                    }
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
            VStack(alignment: .leading, spacing: 10) {
                if !model.attachments.isEmpty {
                    ScrollView(.horizontal, showsIndicators: false) {
                        HStack(spacing: 8) {
                            ForEach(model.attachments) { file in
                                PiAttachmentChip(file: file) { model.attachments.removeAll { $0.id == file.id } }
                            }
                        }
                    }
                }
                TextField("Message Point Guard…", text: $model.draft, axis: .vertical)
                    .textFieldStyle(.plain).font(.system(size: 14)).lineLimit(1...8).focused($composerFocused)
                    .fixedSize(horizontal: false, vertical: true)
                    .padding(.horizontal, 4).frame(minHeight: 34, alignment: .center)
                    .accessibilityLabel("Message draft")
                    .onKeyPress(keys: [.return], phases: .down) { press in
                        if press.modifiers.contains(.shift) { return .ignored }
                        if model.canSend && !importingAttachment { submitDraft() }
                        return .handled
                    }
                HStack(spacing: compact ? 8 : 12) {
                    Button {
                        showFiles = true
                    } label: {
                        Group {
                            if importingAttachment {
                                ProgressView().controlSize(.small)
                            } else {
                                Image(systemName: "plus").font(.system(size: 18))
                            }
                        }.frame(width: 34, height: 34).contentShape(Rectangle())
                    }
                    .buttonStyle(.plain).disabled(importingAttachment || model.attachments.count >= 4)
                    .accessibilityLabel("Add attachment").help("Attach an image, text file, or PDF")
                    Spacer(minLength: 0)
                    if model.busy {
                        Menu {
                            Button("Send after reply") { submitDraft(mode: "after") }
                            Button("Steer current work") { submitDraft(mode: "steer") }
                        } label: {
                            if compact {
                                composerActionIcon("arrow.up", enabled: model.canSend && !importingAttachment)
                            } else {
                                Text("Send options").font(.system(size: 12, weight: .medium))
                            }
                        }
                        .menuStyle(.borderlessButton).fixedSize()
                        .accessibilityLabel("Send options")
                        .disabled(!model.canSend || importingAttachment).help("Choose when to send your draft")
                        Button {
                            Task { _ = await model.stop() }
                        } label: {
                            composerActionIcon("stop.fill", enabled: model.connected)
                        }
                        .buttonStyle(.plain)
                        .accessibilityLabel("Stop Point Guard")
                        .help("Stop Pi and queued messages; delegated workers keep running").disabled(!model.connected)
                    } else {
                        Button {
                            submitDraft()
                        } label: {
                            composerActionIcon("arrow.up", enabled: model.canSend && !importingAttachment)
                        }
                        .buttonStyle(.plain).disabled(!model.canSend || importingAttachment)
                        .accessibilityLabel("Send message").help("Send message (Return)")
                    }
                }
                if model.busy && !model.attachments.isEmpty {
                    Text("Attachments are ready for your next message.")
                        .font(.system(size: 11)).foregroundStyle(.secondary)
                }
            }
            .padding(12)
            .background(
                Color.primary.opacity(0.045),
                in: RoundedRectangle(cornerRadius: PurePointTheme.composerCornerRadius)
            )
            .overlay {
                RoundedRectangle(cornerRadius: PurePointTheme.composerCornerRadius)
                    .strokeBorder(composerFocused ? Color.accentColor.opacity(0.35) : Color.primary.opacity(0.08))
            }
        }
        .frame(maxWidth: PurePointTheme.chatContentWidth).frame(maxWidth: .infinity)
        .padding(.horizontal, compact ? 8 : PurePointTheme.chatHorizontalPadding).padding(.top, 8).padding(.bottom, 16)
        .onAppear { composerFocused = isFocused }
        .onChange(of: isFocused) { _, focused in
            if !focused { composerFocused = false }
        }
    }

    private func importAttachment(_ result: Result<URL, Error>) {
        let draftKey = model.draftKey
        Task {
            importingAttachment = true
            defer { importingAttachment = false }
            do {
                let url = try result.get()
                let file = try await Task.detached(priority: .userInitiated) { try PiAttachmentImport.file(url) }.value
                guard model.draftKey == draftKey else {
                    model.error = "The conversation changed. Attach the file again to your current draft."
                    return
                }
                model.addAttachment(file)
                composerFocused = true
            } catch {
                let failure = error as NSError
                if failure.domain != NSCocoaErrorDomain || failure.code != NSUserCancelledError {
                    model.error = error.localizedDescription
                }
            }
        }
    }

    private func submitDraft(mode: String? = nil) {
        followOutput = true
        model.submit(mode: mode ?? (model.busy ? "after" : "send"))
        composerFocused = true
    }

    private func composerActionIcon(_ name: String, enabled: Bool) -> some View {
        Image(systemName: name).font(.system(size: 14, weight: .semibold))
            .frame(width: 34, height: 34)
            .foregroundStyle(Color(NSColor.controlBackgroundColor))
            .background(enabled ? Color.accentColor : Color.secondary.opacity(0.35), in: Circle())
            .contentShape(Circle())
    }
}

private struct PiAttachmentChip: View {
    let file: PiComposerAttachment
    let remove: () -> Void
    var body: some View {
        HStack(spacing: 8) {
            if file.isImage, let image = NSImage(data: file.data) {
                Image(nsImage: image).resizable().scaledToFill().frame(width: 32, height: 32)
                    .clipShape(RoundedRectangle(cornerRadius: 6)).accessibilityHidden(true)
            } else {
                Image(systemName: "doc.text").frame(width: 24, height: 32).accessibilityHidden(true)
            }
            Text(file.name).font(.system(size: 12)).lineLimit(1).frame(maxWidth: 140, alignment: .leading)
            Button(action: remove) {
                Image(systemName: "xmark").font(.system(size: 10)).frame(width: 24, height: 24)
            }.buttonStyle(.plain).accessibilityLabel("Remove " + file.name)
        }
        .padding(8).background(Color.primary.opacity(0.045), in: RoundedRectangle(cornerRadius: 12))
    }
}

private struct PiToolActivityView: View {
    let tools: [PiToolActivity]
    @State private var expanded = false
    private var running: Int { tools.filter { $0.state == "running" }.count }
    private var failed: Int { tools.filter { $0.state == "failed" }.count }

    var body: some View {
        DisclosureGroup(isExpanded: $expanded) {
            VStack(alignment: .leading, spacing: 0) {
                ForEach(Array(tools.enumerated()), id: \.element.id) { index, tool in
                    if index > 0 { Divider() }
                    PiToolDetailView(tool: tool)
                }
            }.padding(.top, 8).frame(maxWidth: .infinity, alignment: .leading)
        } label: {
            HStack(alignment: .firstTextBaseline, spacing: 10) {
                Text("\(tools.count) tool call\(tools.count == 1 ? "" : "s")")
                    .font(.system(size: 12, weight: .medium))
                if running > 0 {
                    Text("Working").font(.system(size: 11)).foregroundStyle(.secondary)
                } else if failed > 0 {
                    Text("\(failed) failed").font(.system(size: 11)).foregroundStyle(.red)
                }
            }.foregroundStyle(.secondary).frame(maxWidth: .infinity, alignment: .leading)
        }
        .padding(.vertical, 4).frame(minHeight: 36)
        .frame(maxWidth: .infinity, alignment: .leading)
        .accessibilityHint("Expand to inspect tool calls and their output")
    }
}

private struct PiToolDetailView: View {
    let tool: PiToolActivity
    var body: some View {
        DisclosureGroup {
            ScrollView(.horizontal) {
                Text(tool.text.isEmpty ? "No output" : tool.text)
                    .font(.system(size: 12, design: .monospaced)).textSelection(.enabled)
                    .fixedSize(horizontal: true, vertical: true)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(.top, 8).padding(.bottom, 4)
            }
        } label: {
            HStack(alignment: .firstTextBaseline, spacing: 10) {
                Text(tool.name).font(.system(size: 13, weight: .medium))
                    .frame(maxWidth: .infinity, alignment: .leading)
                Text(tool.state == "finished" ? "Done" : tool.state.capitalized)
                    .font(.system(size: 11)).foregroundStyle(.secondary)
            }.foregroundStyle(tool.state == "failed" ? Color.red : Color.secondary)
        }.padding(.vertical, 6).frame(maxWidth: .infinity, alignment: .leading)
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
