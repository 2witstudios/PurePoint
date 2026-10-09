import SwiftUI
import UIKit

struct ChatView: View {
    @StateObject private var model: ChatModel
    @MainActor init(model: ChatModel? = nil) { _model = StateObject(wrappedValue: model ?? ChatModel()) }
    @Environment(\.scenePhase) private var scenePhase
    @State private var showConnection = false
    @State private var showHistory = false
    @State private var confirmNew = false
    @State private var presentedDialog: ExtensionDialog?
    @State private var following = true
    @FocusState private var composerFocused: Bool

    var body: some View {
        NavigationStack {
            VStack(spacing: 0) {
                if let error = model.error {
                    HStack(alignment: .top, spacing: 12) {
                        Image(systemName: "exclamationmark.circle").foregroundStyle(.orange)
                        Text(error).font(.callout).textSelection(.enabled)
                        Button { model.error = nil } label: { Image(systemName: "xmark") }.accessibilityLabel("Dismiss error")
                    }.padding(16).background(Color(uiColor: .secondarySystemBackground))
                }
                transcript
                composer
            }
            .background(Color(uiColor: .systemBackground))
            .navigationTitle("Point Guard")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarLeading) {
                    Button { showHistory = true; Task { await model.loadConversations() } } label: { Image(systemName: "clock.arrow.circlepath") }.accessibilityLabel("Conversations")
                }
                ToolbarItem(placement: .principal) {
                    HStack(spacing: 8) {
                        Image("PurePointLogo").resizable().scaledToFit().frame(width: 26, height: 26).accessibilityHidden(true)
                        VStack(alignment: .leading, spacing: 2) {
                        Text("Point Guard").font(.headline)
                        Text(model.busy ? "Working" : model.connected ? (model.demo ? "Preview" : "Connected") : "Disconnected")
                            .font(.caption2).foregroundStyle(.secondary)
                        }
                    }.accessibilityElement(children: .combine)
                }
                ToolbarItemGroup(placement: .topBarTrailing) {
                    Button { if model.busy { confirmNew = true } else { Task { await model.changeSession() } } } label: { Image(systemName: "square.and.pencil") }.disabled(!model.connected || model.demo || model.changingSession).accessibilityLabel("New conversation")
                    Button { showConnection = true } label: { Image(systemName: model.connected ? "link" : "link.badge.plus") }.accessibilityLabel("Connection settings")
                }
            }
            .confirmationDialog("Stop Pi before starting a new conversation?", isPresented: $confirmNew, titleVisibility: .visible) {
                Button("Stop and start new", role: .destructive) { Task { await model.changeSession(stopFirst: true) } }
                Button("Keep working", role: .cancel) {}
            } message: { Text("Queued messages will be canceled and recoverable. Delegated workers keep running.") }
            .sheet(isPresented: $showConnection, onDismiss: presentPendingDialog) { ConnectionView(model: model) }
            .sheet(isPresented: $showHistory, onDismiss: presentPendingDialog) { ConversationView(model: model) }
            .sheet(item: dialogBinding) { dialog in ExtensionDialogView(model: model, dialog: dialog) }
            .onChange(of: model.snapshot?.dialogs.first?.id) { _, _ in
                if model.snapshot?.dialogs.first != nil && (showHistory || showConnection) { showHistory = false; showConnection = false }
                else { presentPendingDialog() }
            }
            .onChange(of: scenePhase) { _, phase in model.setForeground(phase == .active) }
            .task { if model.endpoint.isEmpty { showConnection = true } else { model.connect() } }
        }
        .tint(.accentColor)
    }
    private func presentPendingDialog() { presentedDialog = model.snapshot?.dialogs.first }
    private var dialogBinding: Binding<ExtensionDialog?> {
        Binding(get: { presentedDialog }, set: { newValue in
            let previous = presentedDialog
            presentedDialog = newValue
            if newValue == nil, let previous, model.snapshot?.dialogs.contains(where: { $0.id == previous.id }) == true { Task { await model.answer(previous, cancelled: true) } }
        })
    }
    private var transcript: some View {
        ScrollViewReader { proxy in
            ZStack(alignment: .bottomTrailing) {
                ScrollView {
                    LazyVStack(alignment: .leading, spacing: 24) {
                        if (model.snapshot?.messages ?? []).isEmpty {
                            Image("PurePointLogo").resizable().scaledToFit().frame(width: 88, height: 88)
                                .frame(maxWidth: .infinity).padding(.top, 64).padding(.bottom, 40).accessibilityHidden(true)
                        }
                        ForEach(TranscriptRows.make(messages: model.snapshot?.messages ?? [], tools: model.snapshot?.tools ?? [])) { row in TranscriptRowView(row: row) }
                        if model.busy { HStack(spacing: 10) { ProgressView().controlSize(.small); Text("Working…").font(.callout).foregroundStyle(.secondary) }.accessibilityElement(children: .combine) }
                        ForEach(Array((model.snapshot?.notices ?? []).enumerated()), id: \.offset) { _, notice in Text(notice).font(.footnote).foregroundStyle(.secondary).textSelection(.enabled) }
                        Color.clear.frame(height: 1).id("bottom").onAppear { following = true }.onDisappear { following = false }
                    }.padding(.horizontal, 20).padding(.vertical, 24)
                }
                .scrollDismissesKeyboard(.interactively)
                .onChange(of: model.snapshot?.revision) { _, _ in if following { proxy.scrollTo("bottom", anchor: .bottom) } }
                .onChange(of: model.snapshot?.sessionId) { _, _ in following = true; proxy.scrollTo("bottom", anchor: .bottom) }
                if !following {
                    Button { following = true; withAnimation { proxy.scrollTo("bottom", anchor: .bottom) } } label: { Label("Latest", systemImage: "arrow.down").font(.callout.weight(.medium)).padding(.horizontal, 14).padding(.vertical, 10).background(.regularMaterial, in: Capsule()) }.padding(16)
                }
            }
        }
    }
    private var composer: some View {
        VStack(alignment: .leading, spacing: 12) {
            if let offer = model.editorOffer {
                HStack { Text("Extension offered a draft").font(.footnote); Spacer(); Button("Add to draft") { model.useEditorOffer() }; Button("Dismiss") { model.editorOffer = nil } }
                    .font(.footnote).accessibilityHint(String(offer.prefix(200)))
            }
            if !model.recoverable.isEmpty {
                DisclosureGroup("Recover messages (\(model.recoverable.count))") {
                    ForEach(model.recoverable) { item in
                        VStack(alignment: .leading, spacing: 8) {
                            Text(item.text).font(.callout).lineLimit(3).textSelection(.enabled)
                            Text(item.status).font(.caption).foregroundStyle(.secondary)
                            HStack { Button("Restore to draft") { model.restore(item) }; Spacer(); Button("Dismiss") { model.dismissSubmission(item.id) } }.font(.footnote)
                        }.padding(.vertical, 8)
                    }
                }.font(.footnote).padding(.bottom, 2)
            }
            if let receipt = model.submissions.last, !receipt.recoverable, receipt.status != "Accepted" {
                Text(receipt.status == "Queued" ? "Queued · waiting for Pi" : receipt.status).font(.caption).foregroundStyle(.secondary)
            }
            if let queue = model.snapshot?.queue, !queue.isEmpty { Text("\(queue.count) message\(queue.count == 1 ? "" : "s") queued").font(.caption).foregroundStyle(.secondary) }
            HStack(alignment: .bottom, spacing: 12) {
                TextField("Message Point Guard", text: $model.draft, axis: .vertical)
                    .font(.body).lineLimit(1...7).focused($composerFocused)
                    .padding(.horizontal, 16).padding(.vertical, 13)
                    .background(Color(uiColor: .secondarySystemBackground), in: RoundedRectangle(cornerRadius: 22))
                    .accessibilityLabel("Message draft")
                if !model.busy {
                    Button { model.submit(mode: "send"); following = true } label: { Image(systemName: "arrow.up").font(.body.weight(.semibold)).frame(width: 46, height: 46).foregroundStyle(Color(uiColor: .systemBackground)).background(model.canSend ? Color.accentColor : Color(uiColor: .tertiaryLabel), in: Circle()) }.disabled(!model.canSend).accessibilityLabel("Send message")
                }
            }
            if model.busy {
                HStack(spacing: 12) {
                    Button("Steer") { model.submit(mode: "steer") }.buttonStyle(.bordered).disabled(!model.canSend)
                    Button("After reply") { model.submit(mode: "after") }.buttonStyle(.bordered).disabled(!model.canSend)
                    Spacer(minLength: 0)
                    Button { Task { _ = await model.stop() } } label: { Label("Stop", systemImage: "stop.fill") }.buttonStyle(.bordered).tint(.secondary).disabled(!model.connected || model.demo)
                }.font(.callout)
            }
            if !model.connected { Button(model.connectionStatus) { showConnection = true }.font(.footnote).foregroundStyle(.secondary) }
        }
        .padding(.horizontal, 16).padding(.top, 12).padding(.bottom, 10)
        .background(.bar)
    }
}

struct MessageView: View {
    let message: ChatMessage
    var body: some View {
        if message.role == "user" {
            HStack { Spacer(minLength: 32); Text(message.text).font(.body).lineSpacing(3).textSelection(.enabled).padding(.horizontal, 17).padding(.vertical, 13).foregroundStyle(.primary).background(Color(uiColor: .secondarySystemBackground), in: RoundedRectangle(cornerRadius: 22)).accessibilityLabel("You: \(message.text)") }
        } else if message.role == "toolResult" {
            ToolGroupView(tools: [ToolActivity(id: message.id, name: message.activity ?? "Tool", state: message.error == nil ? "finished" : "failed", text: message.text + (message.error.map { "\n\n" + $0 } ?? ""))])
        } else if message.role == "notice" {
            Label { Text(message.text).font(.footnote).textSelection(.enabled) } icon: { Image(systemName: "text.alignleft") }.foregroundStyle(.secondary)
        } else {
            VStack(alignment: .leading, spacing: 12) { RichText(text: message.text); if let error = message.error { Text(error).font(.callout).foregroundStyle(.red).textSelection(.enabled) } }
        }
    }
}
struct RichText: View {
    let text: String
    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            ForEach(Array(MarkdownBlocks.split(text).enumerated()), id: \.offset) { _, block in
                switch block {
                case .prose(let prose): Text((try? AttributedString(markdown: prose, options: .init(interpretedSyntax: .inlineOnlyPreservingWhitespace))) ?? AttributedString(prose)).font(.body).lineSpacing(4).textSelection(.enabled).frame(maxWidth: .infinity, alignment: .leading)
                case .code(let language, let code):
                    VStack(alignment: .leading, spacing: 10) {
                        HStack { Text(language.isEmpty ? "Code" : language).font(.caption).foregroundStyle(.secondary); Spacer(); Button { UIPasteboard.general.string = code; UIAccessibility.post(notification: .announcement, argument: "Code copied") } label: { Label("Copy", systemImage: "doc.on.doc").font(.caption) }.accessibilityLabel("Copy code") }
                        ScrollView(.horizontal) { Text(code).font(.system(.callout, design: .monospaced)).textSelection(.enabled).fixedSize(horizontal: true, vertical: false) }
                    }.padding(14).background(Color(uiColor: .secondarySystemBackground), in: RoundedRectangle(cornerRadius: 12))
                }
            }
        }
    }
}
struct ToolView: View {
    let tool: ToolActivity
    var body: some View {
        DisclosureGroup {
            Text(tool.text.isEmpty ? "No output" : tool.text).font(.system(.footnote, design: .monospaced)).textSelection(.enabled).frame(maxWidth: .infinity, alignment: .leading).padding(.top, 8).padding(.bottom, 4)
        } label: {
            HStack(alignment: .firstTextBaseline, spacing: 10) {
                Image(systemName: tool.state == "running" ? "gearshape" : tool.state == "failed" ? "exclamationmark.circle" : "checkmark.circle").frame(width: 20)
                Text(tool.name).font(.subheadline.weight(.medium)).frame(maxWidth: .infinity, alignment: .leading)
                Text(tool.state == "finished" ? "Done" : tool.state.capitalized).font(.caption).foregroundStyle(.secondary)
            }.foregroundStyle(tool.state == "failed" ? Color.red : Color.primary)
        }.padding(.vertical, 6).frame(maxWidth: .infinity, alignment: .leading)
    }
}
struct TranscriptRowView: View {
    let row: TranscriptRow
    var body: some View {
        switch row {
        case .message(let message): MessageView(message: message)
        case .activity(let tools): ToolGroupView(tools: tools)
        }
    }
}
struct ToolGroupView: View {
    let tools: [ToolActivity]
    @State private var expanded = false
    private var running: Int { tools.filter { $0.state == "running" }.count }
    private var failed: Int { tools.filter { $0.state == "failed" }.count }
    var body: some View {
        DisclosureGroup(isExpanded: $expanded) {
            VStack(alignment: .leading, spacing: 0) {
                ForEach(Array(tools.enumerated()), id: \.element.id) { index, tool in
                    if index > 0 { Divider() }
                    ToolView(tool: tool)
                }
            }.padding(.top, 8).frame(maxWidth: .infinity, alignment: .leading)
        } label: {
            HStack(alignment: .firstTextBaseline, spacing: 10) {
                Image(systemName: "wrench.and.screwdriver").frame(width: 20)
                Text("\(tools.count) tool\(tools.count == 1 ? "" : "s")").font(.subheadline.weight(.medium))
                if running > 0 { Text("Working").font(.caption).foregroundStyle(.secondary) }
                else if failed > 0 { Text("\(failed) failed").font(.caption).foregroundStyle(.red) }
            }.foregroundStyle(.primary).frame(maxWidth: .infinity, alignment: .leading)
        }
        .padding(14)
        .background(Color(uiColor: .secondarySystemBackground), in: RoundedRectangle(cornerRadius: 14))
        .frame(maxWidth: .infinity, alignment: .leading)
        .accessibilityHint("Expand to inspect tool calls and their output")
    }
}
#Preview { let model = ChatModel(); model.exploreDemo(); return ChatView(model: model) }
