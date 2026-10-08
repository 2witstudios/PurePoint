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
            .navigationTitle("Pi")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarLeading) {
                    Button { showHistory = true; Task { await model.loadConversations() } } label: { Image(systemName: "clock.arrow.circlepath") }.accessibilityLabel("Conversations")
                }
                ToolbarItem(placement: .principal) {
                    VStack(spacing: 2) {
                        Text("Pi").font(.system(.headline, design: .rounded))
                        Text(model.busy ? "Working on your Mac" : model.connected ? (model.demo ? "Preview" : "Ready when you are") : "Mac disconnected")
                            .font(.caption2).foregroundStyle(.secondary)
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
        .tint(Color(red: 0.145, green: 0.388, blue: 0.922))
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
                        if let title = model.snapshot?.title, title != "Pi" { Text(title).font(.callout.weight(.medium)).foregroundStyle(.secondary).textSelection(.enabled) }
                        if (model.snapshot?.messages ?? []).isEmpty {
                            VStack(alignment: .leading, spacing: 16) {
                                Text("A little room\nto think.").font(.system(.largeTitle, design: .rounded).weight(.semibold)).tracking(-0.7)
                                Text(model.connected ? "Give Pi a complete thought. Your Mac takes it from here." : "Connect to Pi on your Mac to pick up the conversation.")
                                    .font(.body).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
                            }.padding(.top, 56).padding(.bottom, 40)
                        }
                        ForEach(model.snapshot?.messages ?? []) { message in MessageView(message: message) }
                        if !(model.snapshot?.tools ?? []).isEmpty {
                            VStack(alignment: .leading, spacing: 8) { ForEach(model.snapshot?.tools ?? []) { tool in ToolView(tool: tool) } }
                        }
                        if model.busy { HStack(spacing: 10) { ProgressView().controlSize(.small); Text("Pi is working…").font(.callout).foregroundStyle(.secondary) }.accessibilityElement(children: .combine) }
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
            if let receipt = model.submissions.last, !receipt.recoverable {
                Text(receipt.status == "Queued" ? "Queued · waiting for Pi" : receipt.status == "Accepted" ? "Accepted · completion appears in the conversation" : receipt.status).font(.caption).foregroundStyle(.secondary)
            }
            if let queue = model.snapshot?.queue, !queue.isEmpty { Text("\(queue.count) message\(queue.count == 1 ? "" : "s") queued").font(.caption).foregroundStyle(.secondary) }
            HStack(alignment: .bottom, spacing: 12) {
                TextField("Message Pi", text: $model.draft, axis: .vertical)
                    .font(.body).lineLimit(1...7).focused($composerFocused)
                    .padding(.horizontal, 16).padding(.vertical, 13)
                    .background(Color(uiColor: .secondarySystemBackground), in: RoundedRectangle(cornerRadius: 22))
                    .accessibilityLabel("Message draft")
                if !model.busy {
                    Button { model.submit(mode: "send"); following = true } label: { Image(systemName: "arrow.up").font(.body.weight(.semibold)).frame(width: 46, height: 46).foregroundStyle(.white).background(model.canSend ? Color.accentColor : Color.gray, in: Circle()) }.disabled(!model.canSend).accessibilityLabel("Send message")
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
            HStack { Spacer(minLength: 32); Text(message.text).font(.body).lineSpacing(3).textSelection(.enabled).padding(.horizontal, 17).padding(.vertical, 13).foregroundStyle(.white).background(Color.accentColor, in: RoundedRectangle(cornerRadius: 22)).accessibilityLabel("You: \(message.text)") }
        } else if message.role == "toolResult" {
            DisclosureGroup { RichText(text: message.text); if let error = message.error { Text(error).foregroundStyle(.red) } } label: { Label(message.activity ?? "Tool result", systemImage: message.error == nil ? "checkmark.circle" : "exclamationmark.circle").font(.callout).foregroundStyle(.secondary) }
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
            Text(tool.text).font(.system(.footnote, design: .monospaced)).textSelection(.enabled).frame(maxWidth: .infinity, alignment: .leading).padding(.top, 8)
        } label: {
            HStack(spacing: 10) { Image(systemName: tool.state == "running" ? "gearshape" : tool.state == "failed" ? "exclamationmark.circle" : "checkmark.circle"); Text(tool.name); Spacer(); Text(tool.state.capitalized).font(.caption) }.font(.callout).foregroundStyle(tool.state == "failed" ? Color.red : Color.secondary)
        }.padding(.vertical, 4)
    }
}
#Preview { let model = ChatModel(); model.exploreDemo(); return ChatView(model: model) }
