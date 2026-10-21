import SwiftUI
import UIKit
import AVFoundation
import VisionKit
import PhotosUI
import UniformTypeIdentifiers
import PDFKit
import ImageIO

enum AttachmentImport {
    static func image(_ data: Data, name: String) throws -> ComposerAttachment {
        guard data.count <= 20 * 1024 * 1024, let source = CGImageSourceCreateWithData(data as CFData, nil) else { throw ComposerError("Choose an image smaller than 20 MB.") }
        for dimension in [1600, 1200, 800] {
            let options: [CFString: Any] = [kCGImageSourceCreateThumbnailFromImageAlways: true, kCGImageSourceCreateThumbnailWithTransform: true, kCGImageSourceThumbnailMaxPixelSize: dimension]
            guard let image = CGImageSourceCreateThumbnailAtIndex(source, 0, options as CFDictionary) else { throw ComposerError("This image could not be opened.") }
            for quality in [0.8, 0.6, 0.4] {
                if let encoded = UIImage(cgImage: image).jpegData(compressionQuality: CGFloat(quality)), encoded.count <= 256 * 1024 {
                    return ComposerAttachment(id: UUID().uuidString, name: name, mimeType: "image/jpeg", data: encoded)
                }
            }
        }
        throw ComposerError("This image could not be prepared. Choose a smaller image.")
    }
    static func file(_ url: URL) throws -> ComposerAttachment {
        let access = url.startAccessingSecurityScopedResource()
        defer { if access { url.stopAccessingSecurityScopedResource() } }
        let values = try url.resourceValues(forKeys: [.fileSizeKey, .contentTypeKey])
        guard (values.fileSize ?? Int.max) <= 20 * 1024 * 1024 else { throw ComposerError("Choose a file smaller than 20 MB.") }
        let name = String(url.lastPathComponent.prefix(120))
        if values.contentType?.conforms(to: .image) == true { return try image(Data(contentsOf: url), name: name) }
        let text: String
        if values.contentType?.conforms(to: .pdf) == true {
            guard let document = PDFDocument(url: url), document.pageCount <= 30, let extracted = document.string, !extracted.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else { throw ComposerError("Choose a PDF with selectable text and up to 30 pages. For scanned pages, attach images instead.") }
            text = extracted
        } else {
            guard (values.fileSize ?? Int.max) <= 32 * 1024, let decoded = String(data: try Data(contentsOf: url), encoding: .utf8), !decoded.contains("\0") else { throw ComposerError("Choose a UTF-8 text file smaller than 32 KiB.") }
            text = decoded
        }
        guard text.utf8.count <= 32 * 1024 else { throw ComposerError("The extracted text exceeds 32 KiB. Choose a shorter document.") }
        return ComposerAttachment(id: UUID().uuidString, name: name, mimeType: "text/plain", data: Data(text.utf8))
    }
}

struct ConnectionView: View {
    @ObservedObject var model: ChatModel
    @Environment(\.dismiss) private var dismiss
    @State private var enrollment = ""
    @State private var showScanner = false
    @State private var requestingCamera = false
    @State private var renaming: SavedMachine?
    @State private var machineName = ""
    var body: some View {
        NavigationStack {
            Form {
                Section {
                    HStack(spacing: 14) {
                        Image("PurePointLogo").resizable().scaledToFit().frame(width: 44, height: 44).accessibilityHidden(true)
                        VStack(alignment: .leading, spacing: 3) {
                            Text(model.selectedMachine?.name ?? "PurePoint").font(.title3.weight(.semibold)).lineLimit(1)
                            HStack(spacing: 6) {
                                StatusDot(color: model.connected ? Color.green : (model.connectionStatus.hasPrefix("Connecting") ? Color.orange : Color(uiColor: .tertiaryLabel)))
                                Text(model.connectionStatus).lineLimit(2)
                            }.font(.footnote).foregroundStyle(.secondary)
                        }
                    }.padding(.vertical, 6).accessibilityElement(children: .combine)
                    if !model.endpoint.isEmpty {
                        HStack(spacing: 12) {
                            Button {
                                // connect() is a no-op while a socket exists; restart it explicitly.
                                if model.connected { model.disconnect(cancelPairing: false) }
                                model.connect()
                            } label: { Label("Reconnect", systemImage: "arrow.clockwise").frame(maxWidth: .infinity) }
                                .buttonStyle(.bordered)
                            if model.connected {
                                Button(role: .destructive) { model.disconnect() } label: { Label("Disconnect", systemImage: "bolt.horizontal.circle").frame(maxWidth: .infinity) }
                                    .buttonStyle(.bordered)
                            }
                        }.listRowInsets(EdgeInsets(top: 4, leading: 16, bottom: 12, trailing: 16))
                    }
                }
                if !model.machines.isEmpty {
                    Section("Your Macs") {
                        ForEach(model.machines) { machine in
                            let selected = model.selectedMachine?.id == machine.id
                            HStack(spacing: 12) {
                                Button { model.selectMachine(machine.id) } label: {
                                    HStack(spacing: 12) {
                                        Image(systemName: "desktopcomputer").font(.body).foregroundStyle(selected ? Color.accentColor : .secondary).frame(width: 28).accessibilityHidden(true)
                                        VStack(alignment: .leading, spacing: 2) {
                                            Text(machine.name).foregroundStyle(.primary).lineLimit(1)
                                            Text(machine.endpoint).font(.system(.caption2, design: .monospaced)).foregroundStyle(.secondary).lineLimit(1).truncationMode(.middle)
                                        }
                                        Spacer()
                                        if selected { Image(systemName: "checkmark").font(.body.weight(.semibold)).foregroundStyle(Color.accentColor).accessibilityLabel("Selected Mac") }
                                    }.contentShape(Rectangle())
                                }.buttonStyle(.plain)
                                Button { machineName = machine.name; renaming = machine } label: { Image(systemName: "pencil").frame(width: 32, height: 32) }
                                    .buttonStyle(.borderless).accessibilityLabel("Rename " + machine.name)
                            }
                        }
                    }
                }
                Section {
                    Button { Task { await openScanner() } } label: { Label("Scan Mac QR code", systemImage: "qrcode.viewfinder").font(.body.weight(.medium)) }
                        .disabled(requestingCamera || model.pairing)
                    DisclosureGroup("Paste Mac pairing code") {
                        TextField("Enrollment QR contents", text: $enrollment, axis: .vertical).textInputAutocapitalization(.never).autocorrectionDisabled().accessibilityLabel("Enrollment code")
                        Button("Pair") {
                            guard let code = PairingCode.parse(enrollment) else { model.pairingError = "Paste a current Mac enrollment code. Legacy pairing secrets are no longer supported."; return }
                            model.pair(code)
                        }.disabled(model.pairing)
                    }
                    if model.pairing {
                        ProgressView("Pairing with your Mac…")
                        Button("Cancel pairing", role: .cancel) { model.cancelPairing() }
                    }
                    if let error = model.pairingError { Text(error).foregroundStyle(.red).textSelection(.enabled) }
                } header: { Text(model.machines.isEmpty ? "Connect your Mac" : "Connect another Mac") } footer: {
                    Text("Open Connect phone in PurePoint on your Mac, on the same Tailscale network.")
                }
                if let error = model.error { Section { Label { Text(error).textSelection(.enabled) } icon: { Image(systemName: "exclamationmark.triangle.fill").foregroundStyle(.orange) } } }
            }
            .navigationTitle("Connections").navigationBarTitleDisplayMode(.inline)
            .toolbar { ToolbarItem(placement: .topBarTrailing) { Button("Done") { model.cancelPairing(); dismiss() } } }
            .onDisappear { model.cancelPairing() }
            .alert("Mac name", isPresented: Binding(get: { renaming != nil }, set: { if !$0 { renaming = nil } })) {
                TextField("Name", text: $machineName)
                Button("Save") { if let machine = renaming { model.renameMachine(machine.id, name: machineName) }; renaming = nil }
                    .disabled(SavedMachine.normalizedName(machineName).isEmpty)
                Button("Cancel", role: .cancel) { renaming = nil }
            } message: {
                Text("Choose a name to recognize this Mac on your iPhone. Up to 60 characters.")
            }
            .sheet(isPresented: $showScanner) {
                PairingScannerView { code in
                    showScanner = false; model.pair(code)
                }
            }
        }
    }
    @MainActor private func openScanner() async {
        guard DataScannerViewController.isSupported else { model.pairingError = "QR scanning is unavailable on this device. Paste the Mac enrollment code below (including in the simulator)."; return }
        requestingCamera = true
        defer { requestingCamera = false }
        let allowed = await AVCaptureDevice.requestAccess(for: .video)
        guard allowed else { model.pairingError = "Allow camera access for PurePoint in iPhone Settings to scan, or paste the Mac enrollment code."; return }
        guard DataScannerViewController.isAvailable else { model.pairingError = "The camera is unavailable. Try again or paste the Mac enrollment code."; return }
        showScanner = true
    }
}

private struct PairingScannerView: View {
    @Environment(\.dismiss) private var dismiss
    @State private var error: String?
    let onPair: (PairingCode) -> Void
    var body: some View {
        NavigationStack {
            QRScanner(onPair: onPair, onError: { error = $0 })
                .ignoresSafeArea(edges: .bottom)
                .safeAreaInset(edge: .bottom) {
                    if let error { Text(error).font(.callout).padding(20).frame(maxWidth: .infinity).background(.regularMaterial) }
                }
                .navigationTitle("Pair with your Mac").navigationBarTitleDisplayMode(.inline)
                .toolbar { ToolbarItem(placement: .topBarTrailing) { Button("Cancel") { dismiss() } } }
        }
    }
}

private struct QRScanner: UIViewControllerRepresentable {
    let onPair: (PairingCode) -> Void
    let onError: (String) -> Void
    func makeUIViewController(context: Context) -> ScannerController {
        ScannerController(onPair: onPair, onError: onError)
    }
    func updateUIViewController(_ uiViewController: ScannerController, context: Context) {}
    static func dismantleUIViewController(_ uiViewController: ScannerController, coordinator: ()) { uiViewController.scanner.stopScanning() }

    final class ScannerController: UIViewController, DataScannerViewControllerDelegate {
        let scanner = DataScannerViewController(recognizedDataTypes: [.barcode(symbologies: [.qr])], qualityLevel: .accurate, recognizesMultipleItems: false, isHighFrameRateTrackingEnabled: false, isPinchToZoomEnabled: true, isGuidanceEnabled: true, isHighlightingEnabled: true)
        private let onPair: (PairingCode) -> Void
        private let onError: (String) -> Void
        private var finished = false
        private var lastPayload: String?
        init(onPair: @escaping (PairingCode) -> Void, onError: @escaping (String) -> Void) {
            self.onPair = onPair; self.onError = onError
            super.init(nibName: nil, bundle: nil)
        }
        required init?(coder: NSCoder) { fatalError("Not used") }
        override func viewDidLoad() {
            super.viewDidLoad()
            scanner.delegate = self
            addChild(scanner); view.addSubview(scanner.view)
            scanner.view.translatesAutoresizingMaskIntoConstraints = false
            NSLayoutConstraint.activate([scanner.view.topAnchor.constraint(equalTo: view.topAnchor), scanner.view.bottomAnchor.constraint(equalTo: view.bottomAnchor), scanner.view.leadingAnchor.constraint(equalTo: view.leadingAnchor), scanner.view.trailingAnchor.constraint(equalTo: view.trailingAnchor)])
            scanner.didMove(toParent: self)
        }
        override func viewDidAppear(_ animated: Bool) {
            super.viewDidAppear(animated)
            do { try scanner.startScanning() } catch { onError("Could not start the camera. Close this screen and try again, or paste the Mac enrollment code.") }
        }
        override func viewWillDisappear(_ animated: Bool) { scanner.stopScanning(); super.viewWillDisappear(animated) }
        func dataScanner(_ dataScanner: DataScannerViewController, didAdd addedItems: [RecognizedItem], allItems: [RecognizedItem]) { read(addedItems) }
        func dataScanner(_ dataScanner: DataScannerViewController, didUpdate updatedItems: [RecognizedItem], allItems: [RecognizedItem]) { read(updatedItems) }
        func dataScanner(_ dataScanner: DataScannerViewController, didTapOn item: RecognizedItem) { read([item]) }
        func dataScanner(_ dataScanner: DataScannerViewController, becameUnavailableWithError error: DataScannerViewController.ScanningUnavailable) { onError("The camera is unavailable. Close this screen and try again, or paste the Mac enrollment code.") }
        private func read(_ items: [RecognizedItem]) {
            guard !finished else { return }
            for item in items {
                guard case .barcode(let barcode) = item, let payload = barcode.payloadStringValue, payload != lastPayload else { continue }
                lastPayload = payload
                guard let code = PairingCode.parse(payload) else { onError("This code is expired or unsupported. Create a new Connect phone QR in PurePoint on your Mac."); continue }
                finished = true; scanner.stopScanning(); onPair(code); return
            }
        }
    }
}
struct ConversationView: View {
    @ObservedObject var model: ChatModel
    let selection: Int
    var closeOverlay: (() -> Void)? = nil
    @Environment(\.dismiss) private var dismiss
    @State private var confirmResume = false
    @State private var confirmNew = false
    @State private var search = ""
    var body: some View {
        NavigationStack {
            Group {
                if let history = model.browsing {
                    ScrollView { LazyVStack(alignment: .leading, spacing: 24) { ForEach(TranscriptRows.make(messages: history.messages, tools: [])) { TranscriptRowView(row: $0) } }.frame(maxWidth: 720, alignment: .leading).frame(maxWidth: .infinity).padding(20) }
                        .safeAreaInset(edge: .bottom) {
                            Button(model.busy ? "Stop and resume this conversation" : "Resume this conversation") {
                                if model.busy { confirmResume = true } else { Task { await model.changeSession(to: history.sessionId, selection: selection); if model.browsing == nil { finish() } } }
                            }.buttonStyle(NeutralPrimaryButtonStyle()).disabled(!model.connected || model.demo || model.changingSession || !model.chatGrant.canSend).padding().frame(maxWidth: .infinity).background(Color(uiColor: .systemBackground))
                        }
                } else {
                    List {
                        if model.conversations.isEmpty { ContentUnavailableView("No conversations", systemImage: "bubble.left.and.bubble.right") }
                        ForEach(model.conversations.filter { search.isEmpty || $0.title.localizedCaseInsensitiveContains(search) }) { conversation in
                            Button { Task { await model.browse(conversation, selection: selection) } } label: {
                                HistoryRow(title: conversation.title, date: conversation.date.map { String($0.prefix(10)) }, preview: conversation.preview, current: conversation.id == model.snapshot?.sessionId)
                            }
                        }
                    }.searchable(text: $search, prompt: "Find a conversation").refreshable { await model.loadConversations(selection: selection) }
                }
            }
            .safeAreaInset(edge: .top) { if let error = model.error { NoticeBanner(text: error).padding(.horizontal, 12).padding(.vertical, 6).background(.bar) } }
            .navigationTitle(model.browsing?.title ?? "History").navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .bottomBar) { Button("New conversation") { if model.busy { confirmNew = true } else { Task { await model.changeSession(selection: selection); finish() } } }.disabled(!model.connected || model.demo || model.changingSession || !model.chatGrant.canSend) }
                ToolbarItem(placement: .topBarLeading) { if model.browsing != nil { Button("Back") { model.browsing = nil } } }
                ToolbarItem(placement: .topBarTrailing) { Button("Close") { model.browsing = nil; finish() }.keyboardShortcut(.cancelAction) }
            }
            .confirmationDialog("Stop Point Guard before starting a new conversation?", isPresented: $confirmNew, titleVisibility: .visible) {
                Button("Stop and start new", role: .destructive) { Task { await model.changeSession(stopFirst: true, selection: selection); finish() } }
                Button("Keep working", role: .cancel) {}
            } message: { Text("Queued messages remain recoverable. Delegated workers keep running.") }
            .confirmationDialog("Stop Point Guard and resume this conversation?", isPresented: $confirmResume, titleVisibility: .visible) {
                Button("Stop and resume", role: .destructive) { if let history = model.browsing { Task { await model.changeSession(to: history.sessionId, stopFirst: true, selection: selection); if model.browsing == nil { finish() } } } }
                Button("Keep working", role: .cancel) {}
            } message: { Text("Queued messages will be recoverable. Delegated workers keep running.") }
        }
    }
    private func finish() { model.browsing = nil; if let closeOverlay { closeOverlay() } else { dismiss() } }
}
private struct HistoryRow: View {
    let title: String
    let date: String?
    let preview: String?
    let current: Bool
    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            HStack(alignment: .firstTextBaseline, spacing: 8) {
                Text(title).font(.body.weight(.medium)).foregroundStyle(.primary).lineLimit(2)
                Spacer(minLength: 4)
                if let date { Text(date).font(.caption2).foregroundStyle(.secondary).fixedSize() }
            }
            if let preview, !preview.isEmpty { Text(preview).font(.caption).foregroundStyle(.secondary).lineLimit(2) }
            if current {
                Label("Current conversation", systemImage: "checkmark.circle.fill").font(.caption.weight(.semibold)).foregroundStyle(Color.accentColor)
            }
        }.padding(.vertical, 4).accessibilityElement(children: .combine)
    }
}
struct NeutralPrimaryButtonStyle: ButtonStyle {
    @Environment(\.isEnabled) private var enabled
    func makeBody(configuration: Configuration) -> some View {
        configuration.label.font(.body.weight(.semibold))
            .foregroundStyle(Color(uiColor: .systemBackground))
            .padding(.horizontal, 18).padding(.vertical, 14).frame(maxWidth: .infinity)
            .background(Color.accentColor, in: RoundedRectangle(cornerRadius: 16))
            .opacity(enabled ? (configuration.isPressed ? 0.8 : 1) : 0.45)
    }
}
struct ExtensionDialogView: View {
    @ObservedObject var model: ChatModel
    let dialog: ExtensionDialog
    let selection: Int
    @State private var text = ""
    @State private var answering = false
    var body: some View {
        NavigationStack {
            Form {
                if let message = dialog.message, !message.isEmpty { Section { Text(message).textSelection(.enabled) } }
                if dialog.method == "select" {
                    Section { ForEach(Array((dialog.options ?? []).enumerated()), id: \.offset) { index, option in
                        Button(option) {
                            if let ids = dialog.optionIds, ids.indices.contains(index) { answer(optionId: ids[index]) }
                            else if model.isSelected(selection) { model.error = "This dialog has no option ID. Refresh the conversation." }
                        }.disabled(answering)
                    } }
                } else if dialog.method == "confirm" {
                    Section { Button("Allow") { answer(confirmed: true) }; Button("Decline", role: .cancel) { answer(confirmed: false) } }.disabled(answering)
                } else {
                    Section {
                        if dialog.method == "editor" { TextEditor(text: $text).frame(minHeight: 240).accessibilityLabel(dialog.title ?? "Extension editor") }
                        else { TextField(dialog.placeholder ?? "Your answer", text: $text).accessibilityLabel(dialog.title ?? "Extension input") }
                        Button("Send answer") { answer(value: text) }.disabled(answering)
                    }
                }
            }
            .safeAreaInset(edge: .top) { if let error = model.error { Text(error).font(.footnote).foregroundStyle(.red).padding(12).frame(maxWidth: .infinity).background(.bar) } }
            .navigationTitle(dialog.title ?? "Point Guard needs your input").navigationBarTitleDisplayMode(.inline)
            .toolbar { ToolbarItem(placement: .topBarLeading) { Button("Cancel") { answer(cancelled: true) }.disabled(answering) } }
            .onAppear { text = dialog.prefill ?? "" }
        }
    }
    private func answer(value: String? = nil, optionId: String? = nil, confirmed: Bool? = nil, cancelled: Bool = false) {
        answering = true; Task { await model.answer(dialog, value: value, optionId: optionId, confirmed: confirmed, cancelled: cancelled, selection: selection); answering = false }
    }
}
