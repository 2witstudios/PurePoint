import SwiftUI
import UIKit
import AVFoundation
import VisionKit

struct ConnectionView: View {
    @ObservedObject var model: ChatModel
    @Environment(\.dismiss) private var dismiss
    @State private var endpoint = ""
    @State private var secret = ""
    @State private var showScanner = false
    @State private var requestingCamera = false
    var body: some View {
        NavigationStack {
            Form {
                Section {
                    HStack(spacing: 12) {
                        Image("PurePointLogo").resizable().scaledToFit().frame(width: 38, height: 38).accessibilityHidden(true)
                        Text("Point Guard").font(.title2.weight(.semibold))
                    }.padding(.vertical, 8)
                }
                Section {
                    Button { Task { await openScanner() } } label: { Label("Scan Mac QR code", systemImage: "qrcode.viewfinder") }
                        .disabled(requestingCamera)
                    DisclosureGroup("Manual connection") {
                        TextField("Mac address", text: $endpoint).textInputAutocapitalization(.never).autocorrectionDisabled().keyboardType(.URL).accessibilityLabel("Mac endpoint")
                        SecureField("Pairing secret", text: $secret).textInputAutocapitalization(.never).autocorrectionDisabled().accessibilityLabel("Pairing secret")
                        Button("Connect") { model.pair(endpoint: endpoint, secret: secret) }
                    }
                }
                if let error = model.error { Section { Text(error).foregroundStyle(.red).textSelection(.enabled) } }
                Section {
                    HStack { Text(model.connectionStatus); Spacer(); if model.connected && !model.demo { Image(systemName: "checkmark.circle.fill").foregroundStyle(.green) } }
                    if model.connected { Button("Disconnect", role: .destructive) { model.disconnect() } }
                    Button("Reconnect") { model.connect() }.disabled(endpoint.isEmpty)
                }
                Section { Button("Preview") { model.exploreDemo(); dismiss() } }
            }
            .navigationTitle("Connection").navigationBarTitleDisplayMode(.inline)
            .toolbar { ToolbarItem(placement: .topBarTrailing) { Button("Done") { dismiss() } } }
            .onAppear { endpoint = model.endpoint; secret = PairingSecret.read(endpoint: endpoint) }
            .onChange(of: model.connected) { _, connected in if connected && !model.demo { dismiss() } }
            .sheet(isPresented: $showScanner) {
                PairingScannerView { code in
                    endpoint = code.endpoint; secret = code.secret; showScanner = false
                    model.pair(endpoint: endpoint, secret: secret)
                }
            }
        }
    }
    @MainActor private func openScanner() async {
        guard DataScannerViewController.isSupported else { model.error = "QR scanning is unavailable on this device. Use manual entry below (including in the simulator)."; return }
        requestingCamera = true
        defer { requestingCamera = false }
        let allowed = await AVCaptureDevice.requestAccess(for: .video)
        guard allowed else { model.error = "Allow camera access for Pi in iPhone Settings to scan, or enter the address and secret manually."; return }
        guard DataScannerViewController.isAvailable else { model.error = "The camera is unavailable. Try again or use manual entry."; return }
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
            do { try scanner.startScanning() } catch { onError("Could not start the camera. Close this screen and try again, or use manual entry.") }
        }
        override func viewWillDisappear(_ animated: Bool) { scanner.stopScanning(); super.viewWillDisappear(animated) }
        func dataScanner(_ dataScanner: DataScannerViewController, didAdd addedItems: [RecognizedItem], allItems: [RecognizedItem]) { read(addedItems) }
        func dataScanner(_ dataScanner: DataScannerViewController, didUpdate updatedItems: [RecognizedItem], allItems: [RecognizedItem]) { read(updatedItems) }
        func dataScanner(_ dataScanner: DataScannerViewController, didTapOn item: RecognizedItem) { read([item]) }
        func dataScanner(_ dataScanner: DataScannerViewController, becameUnavailableWithError error: DataScannerViewController.ScanningUnavailable) { onError("The camera is unavailable. Close this screen and try again, or use manual entry.") }
        private func read(_ items: [RecognizedItem]) {
            guard !finished else { return }
            for item in items {
                guard case .barcode(let barcode) = item, let payload = barcode.payloadStringValue, payload != lastPayload else { continue }
                lastPayload = payload
                guard let code = PairingCode.parse(payload) else { onError("This isn’t a supported Pi pairing code. Scan the QR created by your Mac’s Pi bridge."); continue }
                finished = true; scanner.stopScanning(); onPair(code); return
            }
        }
    }
}
struct ConversationView: View {
    @ObservedObject var model: ChatModel
    @Environment(\.dismiss) private var dismiss
    @State private var confirmResume = false
    @State private var search = ""
    var body: some View {
        NavigationStack {
            Group {
                if let history = model.browsing {
                    ScrollView { LazyVStack(alignment: .leading, spacing: 24) { ForEach(TranscriptRows.make(messages: history.messages, tools: [])) { TranscriptRowView(row: $0) } }.padding(20) }
                        .safeAreaInset(edge: .bottom) {
                            Button(model.busy ? "Stop and resume this conversation" : "Resume this conversation") {
                                if model.busy { confirmResume = true } else { Task { await model.changeSession(to: history.sessionId); if model.browsing == nil { dismiss() } } }
                            }.buttonStyle(.borderedProminent).disabled(!model.connected || model.demo || model.changingSession).padding().frame(maxWidth: .infinity).background(.bar)
                        }
                } else {
                    List {
                        if model.conversations.isEmpty { ContentUnavailableView("No conversations", systemImage: "bubble.left.and.bubble.right") }
                        ForEach(model.conversations.filter { search.isEmpty || $0.title.localizedCaseInsensitiveContains(search) }) { conversation in
                            Button { Task { await model.browse(conversation) } } label: {
                                VStack(alignment: .leading, spacing: 6) { Text(conversation.title).foregroundStyle(.primary).lineLimit(2); if let date = conversation.date { Text(String(date.prefix(10))).font(.caption).foregroundStyle(.secondary) } }.padding(.vertical, 6)
                            }
                        }
                    }.searchable(text: $search, prompt: "Find a conversation").refreshable { await model.loadConversations() }
                }
            }
            .safeAreaInset(edge: .top) { if let error = model.error { Text(error).font(.footnote).foregroundStyle(.red).padding(12).frame(maxWidth: .infinity).background(.bar) } }
            .navigationTitle(model.browsing?.title ?? "Conversations").navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarLeading) { if model.browsing != nil { Button("Back") { model.browsing = nil } } }
                ToolbarItem(placement: .topBarTrailing) { Button("Done") { model.browsing = nil; dismiss() } }
            }
            .confirmationDialog("Stop Pi and resume this conversation?", isPresented: $confirmResume, titleVisibility: .visible) {
                Button("Stop and resume", role: .destructive) { if let history = model.browsing { Task { await model.changeSession(to: history.sessionId, stopFirst: true); if model.browsing == nil { dismiss() } } } }
                Button("Keep working", role: .cancel) {}
            } message: { Text("Queued messages will be recoverable. Delegated workers keep running.") }
        }
    }
}
struct ExtensionDialogView: View {
    @ObservedObject var model: ChatModel
    let dialog: ExtensionDialog
    @State private var text = ""
    @State private var answering = false
    var body: some View {
        NavigationStack {
            Form {
                if let message = dialog.message, !message.isEmpty { Section { Text(message).textSelection(.enabled) } }
                if dialog.method == "select" {
                    Section { ForEach(Array((dialog.options ?? []).enumerated()), id: \.offset) { _, option in Button(option) { answer(value: option) }.disabled(answering) } }
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
            .navigationTitle(dialog.title ?? "Pi needs your input").navigationBarTitleDisplayMode(.inline)
            .toolbar { ToolbarItem(placement: .topBarLeading) { Button("Cancel") { answer(cancelled: true) }.disabled(answering) } }
            .onAppear { text = dialog.prefill ?? "" }
        }
    }
    private func answer(value: String? = nil, confirmed: Bool? = nil, cancelled: Bool = false) {
        answering = true; Task { await model.answer(dialog, value: value, confirmed: confirmed, cancelled: cancelled); answering = false }
    }
}
