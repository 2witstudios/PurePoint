import SwiftUI
import UIKit
import PhotosUI
import UniformTypeIdentifiers

struct ChatView: View {
    @StateObject private var model: ChatModel
    @MainActor init(model: ChatModel? = nil) { _model = StateObject(wrappedValue: model ?? ChatModel()) }
    @Environment(\.scenePhase) private var scenePhase
    @State private var showConnection = false
    @State private var showHistory = false
    @State private var showSidebar = false
    @State private var showPhotos = false
    @State private var showFiles = false
    @State private var selectedPhoto: PhotosPickerItem?
    @State private var importingAttachment = false
    @State private var confirmNew = false
    @State private var dialogSelection = 0
    @State private var attachmentSelection = 0
    @State private var presentedDialog: ExtensionDialog?
    @State private var following = true
    @State private var draggingTranscript = false
    @State private var transcriptBottom: CGFloat = 0
    @State private var viewportBottom: CGFloat = 0
    @FocusState private var composerFocused: Bool
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

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
            }
            .safeAreaInset(edge: .bottom, spacing: 0) { composer }
            .background(Color(uiColor: .systemBackground))
            .navigationTitle("PurePoint")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarLeading) {
                    Button(action: openSidebar) { Image(systemName: "sidebar.left") }.accessibilityLabel("Open conversations sidebar")
                }
                ToolbarItem(placement: .principal) {
                    Button { showConnection = true } label: {
                        HStack(spacing: 8) {
                            Image("PurePointLogo").resizable().scaledToFit().frame(width: 26, height: 26).accessibilityHidden(true)
                            VStack(alignment: .leading, spacing: 2) {
                                Text("PurePoint").font(.headline)
                                HStack(spacing: 4) {
                                    if let machine = model.selectedMachine {
                                        Text(machine.name).lineLimit(1).truncationMode(.middle)
                                        Text("·")
                                    }
                                    Text(headerStatus).fixedSize(horizontal: true, vertical: false)
                                }.font(.caption2).foregroundStyle(.secondary)
                            }
                        }.frame(minHeight: 44)
                    }
                    .buttonStyle(.plain)
                    .accessibilityElement(children: .combine)
                    .accessibilityHint("Open connection settings")
                }
                ToolbarItemGroup(placement: .topBarTrailing) {
                    Button(action: newConversation) { Image(systemName: "square.and.pencil") }.disabled(!model.connected || model.demo || model.changingSession).accessibilityLabel("New conversation")
                }
            }
            .confirmationDialog("Stop Pi before starting a new conversation?", isPresented: $confirmNew, titleVisibility: .visible) {
                Button("Stop and start new", role: .destructive) { let token = model.selectionToken; Task { await model.changeSession(stopFirst: true, selection: token) } }
                Button("Keep working", role: .cancel) {}
            } message: { Text("Queued messages will be canceled and recoverable. Delegated workers keep running.") }
            .sheet(isPresented: $showConnection, onDismiss: presentPendingDialog) { ConnectionView(model: model) }
            .sheet(isPresented: $showHistory, onDismiss: presentPendingDialog) { ConversationView(model: model, selection: model.selectionToken) }
            .sheet(item: dialogBinding) { dialog in ExtensionDialogView(model: model, dialog: dialog, selection: dialogSelection) }
            .onChange(of: model.snapshot?.dialogs.first?.id) { _, _ in
                if showSidebar { closeSidebar() }
                if model.snapshot?.dialogs.first != nil && (showPhotos || showFiles) {
                    showPhotos = false; showFiles = false
                    let token = model.selectionToken
                    Task { try? await Task.sleep(nanoseconds: 400_000_000); if model.isSelected(token) { presentPendingDialog() } }
                } else if model.snapshot?.dialogs.first != nil && showHistory { showHistory = false }
                else { presentPendingDialog() }
            }
            .onChange(of: model.selectionToken) { _, _ in
                presentedDialog = nil; confirmNew = false; showHistory = false
                showPhotos = false; showFiles = false; selectedPhoto = nil; importingAttachment = false; closeSidebar()
            }
            .onChange(of: scenePhase) { _, phase in model.setForeground(phase == .active) }
            .onAppear { LaunchLog.logger.notice("Chat view appeared") }
            .task { guard !model.demo else { return }; if model.endpoint.isEmpty { showConnection = true } else { model.connect() } }
            .photosPicker(isPresented: $showPhotos, selection: $selectedPhoto, matching: .images)
            .onChange(of: selectedPhoto) { _, item in
                guard let item else { return }
                let token = attachmentSelection
                guard model.isSelected(token) else { return }
                Task {
                    importingAttachment = true
                    defer { if model.isSelected(token) { importingAttachment = false; selectedPhoto = nil } }
                    do {
                        guard let data = try await item.loadTransferable(type: Data.self) else { throw ComposerError("This photo could not be opened.") }
                        let file = try await Task.detached(priority: .userInitiated) { try AttachmentImport.image(data, name: "Photo.jpg") }.value
                        guard model.isSelected(token) else { return }; model.addAttachment(file)
                    } catch { if model.isSelected(token) { model.error = error.localizedDescription } }
                }
            }
            .fileImporter(isPresented: $showFiles, allowedContentTypes: [.image, .plainText, .sourceCode, .json, .pdf], allowsMultipleSelection: false) { result in
                let token = attachmentSelection
                guard model.isSelected(token) else { return }
                Task {
                    importingAttachment = true
                    defer { if model.isSelected(token) { importingAttachment = false } }
                    do {
                        guard let url = try result.get().first else { return }
                        let file = try await Task.detached(priority: .userInitiated) { try AttachmentImport.file(url) }.value
                        guard model.isSelected(token) else { return }; model.addAttachment(file)
                    } catch {
                        let failure = error as NSError
                        if model.isSelected(token) && (failure.domain != NSCocoaErrorDomain || failure.code != NSUserCancelledError) { model.error = error.localizedDescription }
                    }
                }
            }
        }
        .tint(.accentColor)
        .allowsHitTesting(!showSidebar)
        .accessibilityHidden(showSidebar)
        .overlay(alignment: .leading) {
            if showSidebar {
                GeometryReader { geometry in
                    ZStack(alignment: .leading) {
                        Button(action: closeSidebar) { Color.black.opacity(0.35).ignoresSafeArea() }.buttonStyle(.plain).accessibilityLabel("Close conversations sidebar")
                        ConversationSidebar(model: model, selection: model.selectionToken, close: closeSidebar, newConversation: newConversation, select: selectConversation, connection: { closeSidebar(); showConnection = true })
                            .frame(width: min(320, geometry.size.width * 0.84), height: geometry.size.height)
                            .transition(.move(edge: .leading))
                            .simultaneousGesture(DragGesture(minimumDistance: 30).onEnded { value in if value.translation.width < -60 && abs(value.translation.width) > abs(value.translation.height) { closeSidebar() } })
                    }
                }
            }
        }
        .simultaneousGesture(DragGesture(minimumDistance: 30).onEnded { value in
            if !showSidebar && value.startLocation.x < 24 && value.translation.width > 70 && abs(value.translation.width) > abs(value.translation.height) { openSidebar() }
        })
    }
    private var headerStatus: String {
        if model.connected { return model.demo ? "Preview" : model.busy ? "Working" : "Connected" }
        if model.connectionStatus.hasPrefix("Connecting") { return "Connecting" }
        if model.connectionStatus.hasPrefix("Re-pair") { return "Re-pair" }
        if model.connectionStatus.hasPrefix("Paused") { return "Paused" }
        if model.connectionStatus.hasPrefix("Waiting") { return "No network" }
        if model.connectionStatus.hasPrefix("Reconnect paused") { return "Retry paused" }
        if model.connectionStatus.hasPrefix("Connection lost") { return "Offline" }
        return "Not connected"
    }
    private func openSidebar() { let token = model.selectionToken; composerFocused = false; withAnimation(reduceMotion ? nil : .easeOut(duration: 0.22)) { showSidebar = true }; Task { if model.isSelected(token) { await model.loadConversations(selection: token) } } }
    private func closeSidebar() { withAnimation(reduceMotion ? nil : .easeOut(duration: 0.22)) { showSidebar = false } }
    private func newConversation() { let token = model.selectionToken; closeSidebar(); if model.busy { confirmNew = true } else { Task { if model.isSelected(token) { await model.changeSession(selection: token) } } } }
    private func selectConversation(_ conversation: Conversation) {
        let token = model.selectionToken
        closeSidebar()
        Task {
            guard model.isSelected(token) else { return }
            if model.busy || model.demo { await model.browse(conversation, selection: token); if model.isSelected(token) { showHistory = model.browsing != nil } }
            else { await model.changeSession(to: conversation.id, selection: token) }
        }
    }
    private func presentPendingDialog() {
        guard !showConnection, !showHistory, !showPhotos, !showFiles else { return }
        dialogSelection = model.selectionToken; presentedDialog = model.snapshot?.dialogs.first
    }
    private var dialogBinding: Binding<ExtensionDialog?> {
        Binding(get: { presentedDialog }, set: { newValue in
            let previous = presentedDialog
            let token = dialogSelection
            presentedDialog = newValue
            if newValue == nil, let previous, model.snapshot?.dialogs.contains(where: { $0.id == previous.id }) == true { Task { await model.answer(previous, cancelled: true, selection: token) } }
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
                        ForEach(model.transcriptRows) { row in TranscriptRowView(row: row).equatable() }
                        if model.busy { HStack(spacing: 10) { ProgressView().controlSize(.small); Text("Working…").font(.callout).foregroundStyle(.secondary) }.accessibilityElement(children: .combine) }
                        ForEach(Array((model.snapshot?.notices ?? []).enumerated()), id: \.offset) { _, notice in Text(notice).font(.footnote).foregroundStyle(.secondary).textSelection(.enabled) }
                        Color.clear.frame(height: 1).id("bottom")
                            .background(GeometryReader { geometry in
                                Color.clear.preference(key: TranscriptBottomKey.self, value: geometry.frame(in: .global).maxY)
                            })
                    }.frame(maxWidth: 720, alignment: .leading).frame(maxWidth: .infinity).padding(.horizontal, 20).padding(.vertical, 24)
                        .background(GeometryReader { geometry in
                            Color.clear.preference(key: TranscriptHeightKey.self, value: geometry.size.height)
                        })
                }
                .background(GeometryReader { geometry in
                    Color.clear.preference(key: TranscriptViewportKey.self, value: geometry.frame(in: .global).maxY)
                })
                .onPreferenceChange(TranscriptViewportKey.self) { bottom in
                    viewportBottom = bottom
                }
                .onPreferenceChange(TranscriptBottomKey.self) { bottom in
                    guard bottom != 0 else { return } // LazyVStack may omit the offscreen sentinel.
                    let previousBottom = transcriptBottom
                    transcriptBottom = bottom
                    // Resume only when scrolling back to the end, not while a flick
                    // away from the end is still decelerating after DragGesture ends.
                    if !draggingTranscript && bottom < previousBottom && bottom <= viewportBottom + 1 { following = true }
                }
                .onPreferenceChange(TranscriptHeightKey.self) { _ in
                    // Keep following when parsed Markdown changes the reply's height.
                    if following && !draggingTranscript { proxy.scrollTo("bottom", anchor: .bottom) }
                }
                .contentShape(Rectangle())
                .simultaneousGesture(DragGesture().onChanged { value in
                    guard abs(value.translation.height) > abs(value.translation.width) else { return }
                    draggingTranscript = true; following = false
                }.onEnded { value in
                    guard draggingTranscript else { return }
                    draggingTranscript = false
                    // A slow drag back to the end may have no later geometry event.
                    if value.translation.height < 0 && transcriptBottom != 0 && transcriptBottom <= viewportBottom + 1 { following = true }
                })
                .simultaneousGesture(TapGesture().onEnded { composerFocused = false })
                .scrollDismissesKeyboard(.interactively)
                .accessibilityAction(named: "Dismiss keyboard") { composerFocused = false }
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
                            if let files = item.attachments, !files.isEmpty { Text(files.map(\.name).joined(separator: ", ")).font(.caption).foregroundStyle(.secondary) }
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
            VStack(alignment: .leading, spacing: 10) {
                if !model.attachments.isEmpty {
                    ScrollView(.horizontal, showsIndicators: false) {
                        HStack(spacing: 8) {
                            ForEach(model.attachments) { file in
                                AttachmentChip(file: file) { model.attachments.removeAll { $0.id == file.id } }
                            }
                        }
                    }
                }
                TextField("Message PurePoint", text: $model.draft, axis: .vertical)
                    .font(.body).lineLimit(1...6).focused($composerFocused)
                    .padding(.horizontal, 4).padding(.top, 3)
                    .accessibilityLabel("Message draft")
                HStack(spacing: 12) {
                    Menu {
                        Button("Photos", systemImage: "photo") { composerFocused = false; attachmentSelection = model.selectionToken; showPhotos = true }
                        Button("Files", systemImage: "doc") { composerFocused = false; attachmentSelection = model.selectionToken; showFiles = true }
                    } label: {
                        Group { if importingAttachment { ProgressView() } else { Image(systemName: "plus").font(.title3) } }.frame(width: 44, height: 44)
                    }.disabled(importingAttachment || model.attachments.count >= 4).accessibilityLabel("Add attachment")
                    Spacer(minLength: 0)
                    if model.busy {
                        Menu {
                            Button("Steer") { model.submit(mode: "steer"); following = true }
                            Button("After reply") { model.submit(mode: "after"); following = true }
                        } label: { Text("Send options").font(.callout.weight(.medium)).padding(.horizontal, 8).frame(minHeight: 44) }.disabled(!model.canSend || importingAttachment)
                        Button { let token = model.selectionToken; Task { _ = await model.stop(selection: token) } } label: { Image(systemName: "stop.fill").font(.body).frame(width: 40, height: 40).foregroundStyle(Color(uiColor: .systemBackground)).background(Color.accentColor, in: Circle()).frame(width: 44, height: 44).contentShape(Rectangle()) }.disabled(!model.connected || model.demo).accessibilityLabel("Stop")
                    } else {
                        Button { model.submit(mode: "send"); following = true } label: { Image(systemName: "arrow.up").font(.body.weight(.semibold)).frame(width: 40, height: 40).foregroundStyle(Color(uiColor: .systemBackground)).background(model.canSend && !importingAttachment ? Color.accentColor : Color(uiColor: .tertiaryLabel), in: Circle()).frame(width: 44, height: 44).contentShape(Rectangle()) }.disabled(!model.canSend || importingAttachment).accessibilityLabel("Send message")
                    }
                }
                if model.busy && !model.attachments.isEmpty { Text("Attachments are ready for your next message.").font(.caption).foregroundStyle(.secondary) }
            }
            .padding(12)
            .background(Color(uiColor: .secondarySystemBackground), in: RoundedRectangle(cornerRadius: 26))
            .overlay { RoundedRectangle(cornerRadius: 26).stroke(Color(uiColor: .separator).opacity(0.3), lineWidth: 0.5) }
        }
        .padding(.horizontal, 12).padding(.top, 8).padding(.bottom, 8)
    }
}

struct AttachmentChip: View {
    let file: ComposerAttachment
    let remove: () -> Void
    @State private var thumbnail: UIImage?
    var body: some View {
        HStack(spacing: 8) {
            if file.isImage, let image = thumbnail {
                Image(uiImage: image).resizable().scaledToFill().frame(width: 36, height: 36).clipShape(RoundedRectangle(cornerRadius: 8)).accessibilityHidden(true)
            } else { Image(systemName: "doc.text").frame(width: 28, height: 36).accessibilityHidden(true) }
            VStack(alignment: .leading, spacing: 2) { Text(file.name).font(.caption.weight(.medium)).lineLimit(1); Text(file.isImage ? "Image" : "Text").font(.caption2).foregroundStyle(.secondary) }.frame(maxWidth: 140, alignment: .leading)
            Button(action: remove) { Image(systemName: "xmark").font(.caption).frame(width: 44, height: 44) }.accessibilityLabel("Remove \(file.name)")
        }.padding(8).background(Color(uiColor: .tertiarySystemBackground), in: RoundedRectangle(cornerRadius: 14))
        .task(id: file.id) {
            guard file.isImage else { return }
            let data = file.data
            thumbnail = await Task.detached(priority: .utility) { UIImage(data: data)?.preparingForDisplay() }.value
        }
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
            VStack(alignment: .leading, spacing: 12) { RichText(text: message.text).equatable(); if let error = message.error { Text(error).font(.callout).foregroundStyle(.red).textSelection(.enabled) } }
        }
    }
}
struct RichText: View, Equatable {
    let text: String
    nonisolated static func == (lhs: Self, rhs: Self) -> Bool { lhs.text == rhs.text }
    var body: some View { ChatMarkdownView(text: text).equatable() }
}
struct ToolView: View {
    let tool: ToolActivity
    var body: some View {
        DisclosureGroup {
            Text(tool.text.isEmpty ? "No output" : tool.text).font(.system(.footnote, design: .monospaced)).textSelection(.enabled).frame(maxWidth: .infinity, alignment: .leading).padding(.top, 8).padding(.bottom, 4)
        } label: {
            HStack(alignment: .firstTextBaseline, spacing: 10) {
                Text(tool.name).font(.subheadline.weight(.medium)).frame(maxWidth: .infinity, alignment: .leading)
                Text(tool.state == "finished" ? "Done" : tool.state.capitalized).font(.caption).foregroundStyle(.secondary)
            }.foregroundStyle(tool.state == "failed" ? Color.red : Color.secondary)
        }.padding(.vertical, 6).frame(maxWidth: .infinity, alignment: .leading)
    }
}
struct TranscriptRowView: View, Equatable {
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
                Text("\(tools.count) tool call\(tools.count == 1 ? "" : "s")").font(.footnote.weight(.medium))
                if running > 0 { Text("Working").font(.caption).foregroundStyle(.secondary) }
                else if failed > 0 { Text("\(failed) failed").font(.caption).foregroundStyle(.red) }
            }.foregroundStyle(.secondary).frame(maxWidth: .infinity, alignment: .leading)
        }
        .padding(.vertical, 4).frame(minHeight: 44)
        .frame(maxWidth: .infinity, alignment: .leading)
        .accessibilityHint("Expand to inspect tool calls and their output")
    }
}
#Preview { let model = ChatModel(); model.exploreDemo(); return ChatView(model: model) }

// Geometry preferences support iOS 17 without treating streaming layout as a user scroll.
private struct TranscriptBottomKey: PreferenceKey {
    static var defaultValue: CGFloat { 0 }
    static func reduce(value: inout CGFloat, nextValue: () -> CGFloat) { value = nextValue() }
}
private struct TranscriptViewportKey: PreferenceKey {
    static var defaultValue: CGFloat { 0 }
    static func reduce(value: inout CGFloat, nextValue: () -> CGFloat) { value = nextValue() }
}
private struct TranscriptHeightKey: PreferenceKey {
    static var defaultValue: CGFloat { 0 }
    static func reduce(value: inout CGFloat, nextValue: () -> CGFloat) { value = nextValue() }
}
