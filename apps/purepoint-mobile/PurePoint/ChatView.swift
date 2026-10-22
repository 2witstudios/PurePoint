import SwiftUI
import UIKit
import PhotosUI
import UniformTypeIdentifiers

/// Screens pushed over the sidebar home.
enum MobileRoute: Hashable { case pointGuard, history, agent }

/// App root: the desktop sidebar is the home screen; Point Guard, history and agents push over it.
struct ChatView: View {
    @StateObject private var model: ChatModel
    @StateObject private var actions = WorkspaceActions()
    @MainActor init(model: ChatModel? = nil) { _model = StateObject(wrappedValue: model ?? ChatModel()) }
    @Environment(\.scenePhase) private var scenePhase
    @State private var path: [MobileRoute] = []
    @State private var showConnection = false
    @State private var showPhotos = false
    @State private var showFiles = false
    @State private var dialogSelection = 0
    @State private var presentedDialog: ExtensionDialog?

    var body: some View {
        NavigationStack(path: $path) {
            HomeView(
                model: model.agentControl, actions: actions,
                pointGuardStatus: headerStatus, pointGuardTone: headerTone,
                openPointGuard: { path = [.pointGuard] },
                openAgent: openAgent,
                openConnections: { showConnection = true })
            .navigationDestination(for: MobileRoute.self) { route in
                switch route {
                case .pointGuard:
                    PointGuardScreen(model: model, showPhotos: $showPhotos, showFiles: $showFiles,
                                     openHistory: openHistory, openConnection: { showConnection = true })
                case .history:
                    ConversationView(model: model, selection: model.selectionToken, closeOverlay: { if path.last == .history { path.removeLast() } })
                case .agent:
                    AgentScreen(model: model.agentControl, actions: actions, open: openAgent)
                }
            }
        }
        .tint(Theme.accentText)
        .sheet(isPresented: $showConnection, onDismiss: presentPendingDialog) { ConnectionView(model: model) }
        .sheet(item: dialogBinding) { dialog in ExtensionDialogView(model: model, dialog: dialog, selection: dialogSelection) }
        .sheet(item: $actions.review, onDismiss: presentPendingDialog) { launch in LaunchStatusSheet(model: model.agentControl, actions: actions, launch: launch) }
        .onChange(of: model.snapshot?.dialogs.first?.id) { _, _ in
            if model.snapshot?.dialogs.first != nil && (showPhotos || showFiles) {
                showPhotos = false; showFiles = false
                let token = model.selectionToken
                Task { try? await Task.sleep(nanoseconds: 400_000_000); if model.isSelected(token) { presentPendingDialog() } }
            } else { presentPendingDialog() }
        }
        .onChange(of: model.chatGrant) { _, grant in
            if !grant.canRead { path.removeAll { $0 == .history } }
            presentPendingDialog()
        }
        .onChange(of: model.selectionToken) { _, _ in
            presentedDialog = nil; showPhotos = false; showFiles = false; path = []
        }
        .onChange(of: path) { _, _ in updateInventoryVisibility() }
        // Only a real background pauses; .inactive (Notification Center, sheets, app switcher) keeps the agent open.
        .onChange(of: scenePhase) { _, phase in guard phase != .inactive else { return }; model.setForeground(phase == .active); model.agentControl.setInventoryForeground(phase == .active) }
        .task {
            while !Task.isCancelled {
                model.agentControl.updateSignalBadge()
                do { try await Task.sleep(for: .seconds(1)) } catch { return }
            }
        }
        .onAppear { LaunchLog.logger.notice("Chat view appeared"); updateInventoryVisibility() }
        .task { guard !model.demo else { return }; if model.endpoint.isEmpty { showConnection = true } else { model.connect() } }
    }

    private var headerTone: Color {
        if model.connected { return model.busy ? Color(hex: 0x0A84FF) : Theme.online }
        return model.connectionStatus.hasPrefix("Connecting") ? Color(hex: 0xFF9F0A) : Color(uiColor: .tertiaryLabel)
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
    // Inventory polls while the sidebar or an agent is on screen, not under Point Guard.
    private func updateInventoryVisibility() { model.agentControl.setInventoryVisible(path.last == nil || path.last == .agent) }
    private func openAgent(_ machine: AgentMachine, _ agent: ControlledAgent) {
        let control = model.agentControl
        control.open(machine: machine, agent: agent, context: control.contextToken)
        guard control.selected?.agent == agent.id else { return }
        if path.last != .agent { path = [.agent] }
    }
    private func openHistory() {
        guard model.chatGrant.canRead else { return }
        if path.last != .history { path.append(.history) }
        let token = model.selectionToken
        Task { await model.loadConversations(selection: token) }
    }
    private func presentPendingDialog() {
        // Read-only phones cannot answer extension requests; the computer or a Send phone does.
        guard model.chatGrant.canSend else { presentedDialog = nil; return }
        guard !showConnection, !showPhotos, !showFiles, actions.review == nil else { return }
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
}

// MARK: - Point Guard

struct PointGuardScreen: View {
    @ObservedObject var model: ChatModel
    @Binding var showPhotos: Bool
    @Binding var showFiles: Bool
    let openHistory: () -> Void
    let openConnection: () -> Void
    @State private var selectedPhoto: PhotosPickerItem?
    @State private var importingAttachment = false
    @State private var confirmNew = false
    @State private var attachmentSelection = 0
    @State private var following = true
    @State private var draggingTranscript = false
    @State private var transcriptBottom: CGFloat = 0
    @State private var viewportBottom: CGFloat = 0
    @FocusState private var composerFocused: Bool

    var body: some View {
        VStack(spacing: 0) {
            if let error = model.error {
                HStack(alignment: .firstTextBaseline, spacing: 8) {
                    Image(systemName: "exclamationmark.triangle.fill").foregroundStyle(Theme.warning).accessibilityHidden(true)
                    Text(error).font(.footnote).textSelection(.enabled).frame(maxWidth: .infinity, alignment: .leading)
                    Button { model.error = nil } label: { Image(systemName: "xmark").font(.caption.weight(.semibold)).frame(width: 32, height: 32) }.buttonStyle(.plain).accessibilityLabel("Dismiss error")
                }
                .padding(.leading, 12).padding(.trailing, 4).padding(.vertical, 4)
                .background(Theme.warning.opacity(0.12), in: RoundedRectangle(cornerRadius: 10, style: .continuous))
                .padding(.horizontal, 12).padding(.top, 6)
            }
            if model.chatGrant.canRead { transcript } else { chatLocked }
        }
        .safeAreaInset(edge: .bottom, spacing: 0) {
            if model.chatGrant.canSend { composer } else if model.chatGrant.canRead { readOnlyNotice }
        }
        .background(Theme.ground)
        .navigationBarTitleDisplayMode(.inline)
        .toolbarBackground(Theme.ground, for: .navigationBar)
        .toolbar {
            ToolbarItem(placement: .principal) {
                Button(action: openConnection) {
                    VStack(spacing: 1) {
                        Text("Point Guard").font(.headline).foregroundStyle(Theme.ink)
                        HStack(spacing: 4) {
                            StatusDot(color: headerTone, size: 6)
                            if let machine = model.selectedMachine { Text(machine.name).lineLimit(1).truncationMode(.middle); Text("·") }
                            Text(headerStatus).fixedSize(horizontal: true, vertical: false)
                        }.font(.caption2).foregroundStyle(Theme.muted)
                    }.frame(minHeight: 44)
                }
                .buttonStyle(.plain)
                .accessibilityElement(children: .combine)
                .accessibilityHint("Open connection settings")
            }
            ToolbarItemGroup(placement: .topBarTrailing) {
                Button { composerFocused = false; openHistory() } label: { Image(systemName: "clock.arrow.circlepath") }.disabled(!model.chatGrant.canRead).accessibilityLabel("Point Guard history")
                Button(action: newConversation) { Image(systemName: "square.and.pencil") }.disabled(!model.connected || model.demo || model.changingSession || !model.chatGrant.canSend).accessibilityLabel("New conversation")
            }
        }
        .confirmationDialog("Stop Point Guard before starting a new conversation?", isPresented: $confirmNew, titleVisibility: .visible) {
            Button("Stop and start new", role: .destructive) { let token = model.selectionToken; Task { await model.changeSession(stopFirst: true, selection: token) } }
            Button("Keep working", role: .cancel) {}
        } message: { Text("Queued messages will be canceled and recoverable. Delegated workers keep running.") }
        .onChange(of: model.chatGrant) { _, _ in composerFocused = false }
        .onChange(of: model.selectionToken) { _, _ in selectedPhoto = nil; importingAttachment = false; confirmNew = false }
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

    private var headerTone: Color {
        if model.connected { return model.busy ? Color(hex: 0x0A84FF) : Theme.online }
        return model.connectionStatus.hasPrefix("Connecting") ? Color(hex: 0xFF9F0A) : Color(uiColor: .tertiaryLabel)
    }
    private var headerStatus: String {
        if model.connected { return model.demo ? "Preview" : model.busy ? "Working" : "Connected" }
        if model.connectionStatus.hasPrefix("Connecting") { return "Connecting" }
        if model.connectionStatus.hasPrefix("Connection lost") { return "Offline" }
        return "Not connected"
    }
    private var composerStatus: String? {
        var parts: [String] = []
        if let receipt = model.submissions.last, !receipt.recoverable, receipt.status != "Accepted" {
            parts.append(receipt.status == "Queued" ? "Queued · waiting for Point Guard" : receipt.status)
        }
        if let queue = model.snapshot?.queue, !queue.isEmpty { parts.append("\(queue.count) message\(queue.count == 1 ? "" : "s") queued") }
        return parts.isEmpty ? nil : parts.joined(separator: " · ")
    }
    private func newConversation() { let token = model.selectionToken; if model.busy { confirmNew = true } else { Task { if model.isSelected(token) { await model.changeSession(selection: token) } } } }

    private var transcript: some View {
        ScrollViewReader { proxy in
            ZStack(alignment: .bottomTrailing) {
                ScrollView {
                    LazyVStack(alignment: .leading, spacing: 12) {
                        if (model.snapshot?.messages ?? []).isEmpty {
                            PlaysHero(enabled: model.chatGrant.canSend) { prompt in model.draft = prompt; composerFocused = false }
                        }
                        ForEach(model.transcriptRows) { row in TranscriptRowView(row: row).equatable() }
                        if model.busy { TypingIndicatorBubble() }
                        ForEach(Array((model.snapshot?.notices ?? []).enumerated()), id: \.offset) { _, notice in Text(notice).font(.footnote).foregroundStyle(Theme.muted).textSelection(.enabled) }
                        Color.clear.frame(height: 1).id("bottom")
                            .background(GeometryReader { geometry in
                                Color.clear.preference(key: TranscriptBottomKey.self, value: geometry.frame(in: .global).maxY)
                            })
                    }.frame(maxWidth: 720, alignment: .leading).frame(maxWidth: .infinity).padding(.horizontal, 14).padding(.vertical, 16)
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
                    Button { following = true; withAnimation { proxy.scrollTo("bottom", anchor: .bottom) } } label: {
                        Label("Latest", systemImage: "arrow.down").font(.callout.weight(.semibold)).foregroundStyle(Theme.ink)
                            .padding(.horizontal, 14).padding(.vertical, 9).background(Theme.raised, in: Capsule())
                            .shadow(color: .black.opacity(0.4), radius: 12, y: 6)
                    }.padding(16)
                }
            }
        }
    }
    private var chatLocked: some View {
        VStack(spacing: 12) {
            Image(systemName: "lock").font(.title).foregroundStyle(Theme.muted)
                .frame(width: 72, height: 72).background(Color(hex: 0x1F1F23), in: Circle())
                .overlay(Circle().stroke(Color.white.opacity(0.09))).accessibilityHidden(true)
            Text("Point Guard chat is locked").font(.headline)
            Text(model.chatGrant.explanation ?? "").font(.subheadline).foregroundStyle(Theme.muted).multilineTextAlignment(.center)
        }
        .padding(40).frame(maxWidth: .infinity, maxHeight: .infinity)
        .accessibilityElement(children: .combine)
    }
    private var readOnlyNotice: some View {
        HStack(alignment: .firstTextBaseline, spacing: 8) {
            Image(systemName: "eye").foregroundStyle(Theme.muted).accessibilityHidden(true)
            Text(model.chatGrant.explanation ?? "").font(.footnote).foregroundStyle(Theme.muted).frame(maxWidth: .infinity, alignment: .leading)
        }
        .padding(12)
        .background(Theme.surface, in: RoundedRectangle(cornerRadius: 16))
        .padding(.horizontal, 12).padding(.top, 8).padding(.bottom, 8)
        .accessibilityElement(children: .combine)
    }
    private var composer: some View {
        VStack(alignment: .leading, spacing: 8) {
            if let offer = model.editorOffer {
                HStack { Text("Extension offered a draft").font(.footnote); Spacer(); Button("Add to draft") { model.useEditorOffer() }; Button("Dismiss") { model.editorOffer = nil } }
                    .font(.footnote).padding(.horizontal, 8).accessibilityHint(String(offer.prefix(200)))
            }
            if !model.recoverable.isEmpty {
                DisclosureGroup("Recover messages (\(model.recoverable.count))") {
                    ForEach(model.recoverable) { item in
                        VStack(alignment: .leading, spacing: 8) {
                            Text(item.text).font(.callout).lineLimit(3).textSelection(.enabled)
                            if let files = item.attachments, !files.isEmpty { Text(files.map(\.name).joined(separator: ", ")).font(.caption).foregroundStyle(Theme.muted) }
                            Text(item.status).font(.caption).foregroundStyle(Theme.muted)
                            HStack { Button("Restore to draft") { model.restore(item) }.foregroundStyle(Theme.accentText); Spacer(); Button("Dismiss") { model.dismissSubmission(item.id) }.foregroundStyle(Theme.muted) }.font(.footnote.weight(.semibold))
                        }.padding(.vertical, 8)
                    }
                }
                .font(.footnote.weight(.semibold)).tint(Theme.warning).foregroundStyle(Theme.warning)
                .padding(.horizontal, 14).padding(.vertical, 10)
                .background(Theme.surface, in: RoundedRectangle(cornerRadius: 14, style: .continuous))
            }
            if let status = composerStatus { Text(status).font(.caption).foregroundStyle(Theme.muted).padding(.horizontal, 8) }
            VStack(alignment: .leading, spacing: 8) {
                if !model.attachments.isEmpty {
                    ScrollView(.horizontal, showsIndicators: false) {
                        HStack(spacing: 6) {
                            ForEach(model.attachments) { file in
                                AttachmentChip(file: file) { model.attachments.removeAll { $0.id == file.id } }
                            }
                        }
                    }
                }
                TextField("Call it…", text: $model.draft, axis: .vertical)
                    .font(.body).lineLimit(1...6).focused($composerFocused)
                    .padding(.horizontal, 4).padding(.top, 3)
                    .accessibilityLabel("Message draft")
                HStack(spacing: 6) {
                    Menu {
                        Button("Photos", systemImage: "photo") { composerFocused = false; attachmentSelection = model.selectionToken; showPhotos = true }
                        Button("Files", systemImage: "doc") { composerFocused = false; attachmentSelection = model.selectionToken; showFiles = true }
                    } label: {
                        Group { if importingAttachment { ProgressView() } else { Image(systemName: "plus").font(.body.weight(.medium)) } }.foregroundStyle(Theme.muted).frame(width: 40, height: 40)
                    }.disabled(importingAttachment || model.attachments.count >= 4).accessibilityLabel("Add attachment")
                    Spacer(minLength: 0)
                    if model.busy {
                        Menu {
                            Button("Steer now") { model.submit(mode: "steer"); following = true }
                            Button("After reply") { model.submit(mode: "after"); following = true }
                        } label: {
                            Text("Steer").font(.subheadline.weight(.semibold)).foregroundStyle(Theme.accentText)
                                .padding(.horizontal, 14).frame(minHeight: 36).background(Theme.accent.opacity(0.16), in: Capsule()).frame(minHeight: 44)
                        }.disabled(!model.canSend || importingAttachment)
                        Button { let token = model.selectionToken; Task { _ = await model.stop(selection: token) } } label: {
                            RoundedRectangle(cornerRadius: 2).fill(Theme.ground).frame(width: 12, height: 12)
                                .frame(width: 40, height: 40).background(Theme.ink, in: Circle()).frame(width: 44, height: 44).contentShape(Rectangle())
                        }.disabled(!model.connected || model.demo).accessibilityLabel("Stop")
                    } else {
                        let ready = model.canSend && !importingAttachment
                        Button { model.submit(mode: "send"); following = true } label: {
                            Image(systemName: "arrow.up").font(.body.weight(.bold)).frame(width: 40, height: 40)
                                .foregroundStyle(Theme.ground)
                                .background(ready ? Theme.accent : Theme.ink.opacity(0.85), in: Circle())
                                .frame(width: 44, height: 44).contentShape(Rectangle())
                        }.disabled(!ready).accessibilityLabel("Send message")
                    }
                }
                if model.busy && !model.attachments.isEmpty { Text("Attachments are ready for your next message.").font(.caption).foregroundStyle(Theme.muted) }
            }
            .padding(.leading, 14).padding(.trailing, 8).padding(.vertical, 10)
            .background(Theme.surface, in: RoundedRectangle(cornerRadius: 20, style: .continuous))
            .overlay { RoundedRectangle(cornerRadius: 20, style: .continuous).stroke(Color.white.opacity(0.09), lineWidth: 1) }
        }
        .padding(.horizontal, 10).padding(.top, 8).padding(.bottom, 8)
    }
}

/// New-conversation court: four plays and a ball you can drag onto one to call it.
struct PlaysHero: View {
    let enabled: Bool
    let call: (String) -> Void
    @State private var ball: CGPoint?
    @State private var dragging = false
    @State private var target: Int?
    @State private var passed: Int?
    @State private var pulse = false
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    private struct Play { let label: String; let icon: String; let prompt: String }
    // Same built-in plays as the desktop court (PointGuardPlay.builtIns), slot order left/right wing, left/right block.
    private static let plays = [
        Play(label: "Catch me up", icon: "square.stack.3d.up", prompt: "Give me an update on all my projects — what moved, what's blocked, and what needs me next."),
        Play(label: "What's next?", icon: "safari", prompt: "Look across my projects and tell me what I should work on next, and why."),
        Play(label: "Tidy my desktop", icon: "square.grid.2x2", prompt: "Organize my desktop — group the files into folders that make sense and clear the clutter. Show me the plan before you move anything."),
        Play(label: "What's running?", icon: "waveform.path.ecg", prompt: "What's running on my machine right now? Agents, apps, and anything using a lot of memory or CPU."),
    ]
    private static let height: CGFloat = 470

    var body: some View {
        GeometryReader { geometry in
            let width = geometry.size.width
            let logo = CGPoint(x: width / 2, y: 64)
            let rest = CGPoint(x: width / 2 + 40, y: 106)
            let spots = [CGPoint(x: width * 0.27, y: 318), CGPoint(x: width * 0.73, y: 318), CGPoint(x: width * 0.27, y: 420), CGPoint(x: width * 0.73, y: 420)]
            let position = ball ?? (passed.map { spots[$0] } ?? rest)
            let engaged = dragging || passed != nil
            let lean = engaged ? max(-8, min(8, (position.x / width * 100 - 50) * 0.18)) : 0
            let flip: CGFloat = engaged && position.x < width * 0.4 ? -1 : 1
            ZStack {
                Court(center: logo).stroke(Color.white.opacity(0.065), lineWidth: 1.25)
                if dragging {
                    Path { path in path.move(to: rest); path.addLine(to: target.map { spots[$0] } ?? position) }
                        .stroke(target == nil ? Color.white.opacity(0.22) : Theme.accent, style: StrokeStyle(lineWidth: target == nil ? 1 : 1.75, lineCap: .round, dash: target == nil ? [2, 7] : []))
                }
                LogoRing(size: 96)
                    .rotation3DEffect(.degrees(flip < 0 ? 180 : 0), axis: (x: 0, y: 1, z: 0))
                    .rotationEffect(.degrees(lean))
                    .animation(.spring(response: 0.4, dampingFraction: 0.8), value: flip)
                    .position(logo)
                Text("What play do you\nwant to run?").font(.system(size: 30, weight: .bold)).tracking(-0.6)
                    .multilineTextAlignment(.center).foregroundStyle(Color(hex: 0xF4F4F5))
                    .position(x: width / 2, y: 178)
                ForEach(Self.plays.indices, id: \.self) { index in
                    spot(index, lit: index == (target ?? passed)).position(x: spots[index].x, y: spots[index].y + 12)
                }
                Circle()
                    .fill(RadialGradient(colors: [Color.white.opacity(0.55), Theme.accent], center: UnitPoint(x: 0.35, y: 0.3), startRadius: 0, endRadius: 9))
                    .frame(width: 18, height: 18)
                    .scaleEffect(dragging ? 1.25 : 1)
                    .background(Circle().fill(Theme.accent.opacity(pulse ? 0 : 0.45)).scaleEffect(pulse ? 1.9 : 1))
                    .frame(width: 44, height: 44).contentShape(Circle())
                    .opacity(passed != nil && !dragging ? 0 : 1)
                    .position(position)
                    .gesture(DragGesture(minimumDistance: 0, coordinateSpace: .named("court"))
                        .onChanged { value in
                            guard enabled else { return }
                            dragging = true; passed = nil; ball = value.location
                            target = spots.indices.min { distance(spots[$0], value.location) < distance(spots[$1], value.location) }
                                .flatMap { distance(spots[$0], value.location) < 64 ? $0 : nil }
                        }
                        .onEnded { _ in
                            dragging = false
                            withAnimation(.spring(response: 0.45, dampingFraction: 0.8)) {
                                if let target { pass(target) } else { ball = nil }
                                target = nil
                            }
                        })
                    .accessibilityHidden(true)
            }
            .coordinateSpace(name: "court")
        }
        .frame(height: Self.height)
        .onAppear { guard !reduceMotion else { return }; withAnimation(.easeInOut(duration: 1.6).repeatForever(autoreverses: false)) { pulse = true } }
    }

    private func distance(_ a: CGPoint, _ b: CGPoint) -> CGFloat { hypot(a.x - b.x, a.y - b.y) }
    private func pass(_ index: Int) { ball = nil; passed = index; call(Self.plays[index].prompt) }
    private func spot(_ index: Int, lit: Bool) -> some View {
        let play = Self.plays[index]
        return Button { withAnimation(.spring(response: 0.45, dampingFraction: 0.8)) { pass(index) } } label: {
            VStack(spacing: 10) {
                Image(systemName: play.icon).font(.system(size: 16, weight: .medium))
                    .foregroundStyle(lit ? Theme.ground : Theme.muted)
                    .frame(width: 44, height: 44)
                    .background(lit ? Theme.accent : Theme.surface, in: Circle())
                    .overlay(Circle().stroke(lit ? Color.clear : Color.white.opacity(0.16), lineWidth: 1.5))
                    .shadow(color: lit ? Theme.accent.opacity(0.5) : .clear, radius: 14)
                    .scaleEffect(lit ? 1.08 : 1)
                Text(play.label).font(.system(size: 13, weight: lit ? .semibold : .medium)).foregroundStyle(lit ? Theme.accent : Theme.muted).fixedSize()
            }
        }
        .buttonStyle(.plain).disabled(!enabled)
        .animation(.easeOut(duration: 0.2), value: lit)
        .accessibilityLabel(play.label).accessibilityHint(play.prompt)
    }
}

/// Faint half-court lines behind the plays, centered on the logo.
private struct Court: Shape {
    let center: CGPoint
    func path(in rect: CGRect) -> Path {
        var path = Path()
        path.move(to: CGPoint(x: rect.minX, y: center.y)); path.addLine(to: CGPoint(x: rect.maxX, y: center.y))
        path.addEllipse(in: CGRect(x: center.x - 86, y: center.y - 86, width: 172, height: 172))
        let arc = CGPoint(x: rect.midX, y: rect.maxY + 20)
        path.move(to: CGPoint(x: rect.minX + 26, y: rect.maxY)); path.addLine(to: CGPoint(x: rect.minX + 26, y: arc.y - 10))
        path.addArc(center: arc, radius: rect.midX - 26, startAngle: .degrees(180), endAngle: .degrees(360), clockwise: false)
        path.addLine(to: CGPoint(x: rect.maxX - 26, y: rect.maxY))
        return path
    }
}

struct AttachmentChip: View {
    let file: ComposerAttachment
    let remove: () -> Void
    @State private var thumbnail: UIImage?
    var body: some View {
        HStack(spacing: 6) {
            if file.isImage, let image = thumbnail {
                Image(uiImage: image).resizable().scaledToFill().frame(width: 24, height: 24).clipShape(RoundedRectangle(cornerRadius: 5)).accessibilityHidden(true)
            }
            Text(file.isImage ? "IMAGE" : "TEXT").font(.system(size: 10, weight: .semibold)).tracking(0.3).foregroundStyle(Theme.muted)
            Text(file.name).font(.caption).lineLimit(1).frame(maxWidth: 120, alignment: .leading)
            Button(action: remove) { Image(systemName: "xmark").font(.system(size: 10, weight: .bold)).foregroundStyle(Theme.muted).frame(width: 30, height: 30).contentShape(Rectangle()) }.buttonStyle(.plain).accessibilityLabel("Remove \(file.name)")
        }.padding(.leading, 10).frame(minHeight: 32).background(Color(hex: 0x2E2E33), in: RoundedRectangle(cornerRadius: 9))
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
            ChatBubble(sender: .user, userFill: Theme.bubble, userForeground: Theme.ink) { Text(message.text).font(.body).lineSpacing(3).textSelection(.enabled) }.accessibilityElement(children: .combine).accessibilityLabel("You: \(message.text)")
        } else if message.role == "toolResult" {
            ToolGroupView(tools: [ToolActivity(id: message.id, name: message.activity ?? "Tool", state: message.error == nil ? "finished" : "failed", text: message.text + (message.error.map { "\n\n" + $0 } ?? ""))])
        } else if message.role == "notice" {
            Label { Text(message.text).font(.footnote).textSelection(.enabled) } icon: { Image(systemName: "text.alignleft") }.foregroundStyle(Theme.muted)
        } else {
            ChatBubble(sender: .assistant, assistantFill: .clear, assistantTrailingInset: 4) { VStack(alignment: .leading, spacing: 12) { RichText(text: message.text).equatable(); if let error = message.error { Text(error).font(.callout).foregroundStyle(Theme.danger).textSelection(.enabled) } } }
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
                Text(tool.name).font(.system(.footnote, design: .monospaced).weight(.medium)).frame(maxWidth: .infinity, alignment: .leading)
                Text(tool.state == "finished" ? "Done" : tool.state.capitalized).font(.caption)
            }.foregroundStyle(tool.state == "failed" ? Theme.danger : Theme.muted)
        }.tint(Theme.muted).padding(.vertical, 6).frame(maxWidth: .infinity, alignment: .leading)
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
                Text("\(tools.count) tool call\(tools.count == 1 ? "" : "s")").font(.system(.caption, design: .monospaced).weight(.medium))
                if running > 0 { Text("Working").font(.caption) }
                else if failed > 0 { Text("\(failed) failed").font(.caption).foregroundStyle(Theme.danger) }
            }.foregroundStyle(Theme.muted).frame(maxWidth: .infinity, alignment: .leading)
        }
        .tint(Theme.muted)
        .padding(.vertical, 2).frame(minHeight: 40)
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
