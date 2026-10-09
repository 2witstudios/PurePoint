import Foundation
import SwiftUI

@MainActor final class ChatModel: ObservableObject {
    @Published var snapshot: Snapshot? { didSet { if !projectingSnapshot, let snapshot { transcriptRows = TranscriptRows.make(messages: snapshot.messages, tools: snapshot.tools) } else if snapshot == nil { transcriptRows = [] } } }
    private(set) var transcriptRows: [TranscriptRow] = []
    @Published var connected = false
    @Published var connectionStatus = "Not connected"
    @Published var error: String?
    @Published var draft = "" { didSet { saveDraft() } }
    @Published var attachments: [ComposerAttachment] = [] { didSet { saveAttachmentDraft() } }
    @Published var submissions: [Submission] = [] { didSet { saveSubmissions() } }
    @Published var conversations: [Conversation] = []
    @Published var browsing: History?
    @Published var editorOffer: String?
    @Published var changingSession = false
    @Published var endpoint = UserDefaults.standard.string(forKey: "pi.endpoint") ?? ""
    private var socket: URLSessionWebSocketTask?
    private var receiveTask: Task<Void, Never>?
    private var retryTask: Task<Void, Never>?
    private var pending: [String: CheckedContinuation<JSONValue, Error>] = [:]
    private var timeouts: [String: Task<Void, Never>] = [:]
    private var drafts = UserDefaults.standard.dictionary(forKey: "pi.drafts") as? [String: String] ?? [:]
    private var attachmentDrafts: [String: [ComposerAttachment]] = [:]
    private let writer = CoalescedWriter()
    private var recoveryTask: Task<Void, Never>?
    private var connectionTask: Task<Void, Never>?
    private var recoveryLoaded = false
    private var projectingSnapshot = false
    private var cursor = SnapshotCursor()
    private var seenEditorIds = Set<String>()
    private var seenCanceledIds = Set(UserDefaults.standard.stringArray(forKey: "pi.canceledIds") ?? [])
    private var generation = 0
    private var foreground = true
    private var wantsConnection = false
    private var retryDelay: UInt64 = 2
    var demo = false
    var busy: Bool { snapshot?.busy ?? false }
    var canSend: Bool { connected && snapshot?.error == nil && (!demo || !busy) && !changingSession && (!busy || attachments.isEmpty) && (!draft.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || !attachments.isEmpty) }
    var draftKey: String { endpoint + ":" + (snapshot?.sessionId ?? "local") }
    var recoverable: [Submission] { submissions.filter { $0.recoverable } }

    init() {
        draft = drafts[draftKey] ?? ""
        recoveryTask = Task { [weak self] in
            let saved = await Task.detached(priority: .utility) {
                let legacy = UserDefaults.standard.data(forKey: "pi.submissions").flatMap { try? JSONDecoder().decode([Submission].self, from: $0) }
                return (LocalRecoveryStore.load("attachment-drafts", as: [String: [ComposerAttachment]].self) ?? [:], LocalRecoveryStore.load("submissions", as: [Submission].self) ?? legacy ?? [])
            }.value
            guard let self else { return }
            // Edits made while loading always win over saved recovery.
            self.attachmentDrafts = saved.0.merging(self.attachmentDrafts) { _, current in current }
            let currentIds = Set(self.submissions.map(\.id))
            self.submissions = saved.1.filter { !currentIds.contains($0.id) }.map { item in
                var item = item
                if item.status == "Sending" { item.status = "Delivery uncertain"; item.recoverable = true }
                return item
            } + self.submissions
            self.attachments = self.attachmentDrafts[self.draftKey] ?? self.attachments
            self.recoveryLoaded = true
            self.saveSubmissions(); self.saveAttachmentDraft()
        }
    }
    private func saveDraft() {
        drafts[draftKey] = String(draft.prefix(65536))
        if drafts.count > 100 { drafts.removeValue(forKey: drafts.keys.first(where: { $0 != draftKey }) ?? "") }
        let value = drafts
        writer.schedule(key: "drafts") { UserDefaults.standard.set(value, forKey: "pi.drafts") }
    }
    private func saveSubmissions() {
        guard recoveryLoaded else { return }
        let value = Array(submissions.suffix(50))
        writer.schedule(key: "submissions") { [weak self] in
            do { try LocalRecoveryStore.save(value, name: "submissions"); UserDefaults.standard.removeObject(forKey: "pi.submissions") }
            catch { Task { @MainActor [weak self] in self?.error = "Could not save message recovery on this phone." } }
        }
    }
    private func saveAttachmentDraft() {
        attachmentDrafts[draftKey] = attachments
        while attachmentDrafts.count > 10, let key = attachmentDrafts.keys.first(where: { $0 != draftKey }) { attachmentDrafts.removeValue(forKey: key) }
        guard recoveryLoaded else { return }
        let value = attachmentDrafts
        writer.schedule(key: "attachments") { [weak self] in
            do { try LocalRecoveryStore.save(value, name: "attachment-drafts") }
            catch { Task { @MainActor [weak self] in self?.error = "Could not save attachments on this phone. Keep the app open until sending." } }
        }
    }
    func addAttachment(_ file: ComposerAttachment) {
        guard attachments.count < 4, attachments.reduce(file.data.count, { $0 + $1.data.count }) <= 512 * 1024 else { error = "Attach up to four files, totaling 512 KiB after preparation."; return }
        attachments.append(file)
    }
    func pair(endpoint: String, secret: String) {
        guard let url = ConnectionAddress.url(endpoint) else { error = "Enter a Tailscale address ending in /v1, such as ws://100.100.1.2:8787/v1."; return }
        guard secret.count >= 32 else { error = "Enter the pairing secret from your Mac (at least 32 characters)."; return }
        do { try PairingSecret.save(secret, endpoint: url.absoluteString) } catch { self.error = error.localizedDescription; return }
        saveDraft(); saveAttachmentDraft(); disconnect(); self.endpoint = url.absoluteString; UserDefaults.standard.set(self.endpoint, forKey: "pi.endpoint"); snapshot = nil; draft = drafts[draftKey] ?? ""; attachments = attachmentDrafts[draftKey] ?? []; demo = false; wantsConnection = true; connect()
    }
    func connect() {
        guard !demo, foreground, socket == nil, let url = ConnectionAddress.url(endpoint) else { return }
        guard connectionTask == nil else { return }
        wantsConnection = true
        let address = endpoint
        connectionStatus = "Connecting to your Mac…"
        connectionTask = Task { [weak self] in
            await self?.recoveryTask?.value
            let secret = await Task.detached(priority: .userInitiated) { PairingSecret.read(endpoint: address) }.value
            guard !Task.isCancelled, let self else { return }
            self.connectionTask = nil
            guard self.foreground, self.wantsConnection, self.endpoint == address, self.socket == nil else { return }
            self.startConnection(url: url, secret: secret)
        }
    }
    private func startConnection(url: URL, secret: String) {
        guard !secret.isEmpty else { error = "Add the pairing secret in Connection settings."; return }
        wantsConnection = true; retryTask?.cancel(); generation += 1; let current = generation
        cursor.reset(); connectionStatus = "Connecting to your Mac…"; error = nil
        var request = URLRequest(url: url); request.setValue("Bearer " + secret, forHTTPHeaderField: "Authorization")
        let task = URLSession.shared.webSocketTask(with: request); task.maximumMessageSize = 4 * 1024 * 1024; socket = task; task.resume()
        receiveTask = Task { [weak self] in
            do {
                while !Task.isCancelled {
                    let message = try await task.receive()
                    guard let self, current == self.generation else { return }
                    let data: Data
                    switch message { case .string(let text): data = Data(text.utf8); case .data(let bytes): data = bytes; @unknown default: throw MobileError("Unsupported message from bridge.") }
                    let record = try await Task.detached(priority: .userInitiated) { try IncomingRecord.decode(data) }.value
                    guard current == self.generation else { return }
                    try self.receive(record)
                }
            } catch { guard let self, current == self.generation else { return }; self.connectionLost(error.localizedDescription) }
        }
        Task { [weak self] in
            guard let self else { return }
            do {
                try await self.syncState()
                guard current == self.generation else { return }
                self.connected = true; self.connectionStatus = "Connected to your Mac"; self.retryDelay = 2
            }
            catch { if current == self.generation { self.connectionLost(error.localizedDescription) } }
        }
    }
    func setForeground(_ active: Bool) { foreground = active; if active { if wantsConnection || !endpoint.isEmpty { connect() } } else { writer.flush(); detach(); connectionStatus = "Paused on this phone · Pi continues on your Mac" } }
    func disconnect() { wantsConnection = false; retryTask?.cancel(); detach(); connectionStatus = "Not connected" }
    private func detach() {
        generation += 1; connectionTask?.cancel(); connectionTask = nil; connected = false; receiveTask?.cancel(); receiveTask = nil; socket?.cancel(with: .goingAway, reason: nil); socket = nil
        let outstanding = pending; pending = [:]; for timeout in timeouts.values { timeout.cancel() }; timeouts = [:]
        for continuation in outstanding.values { continuation.resume(throwing: MobileError("Connection interrupted. Delivery may be uncertain. Inspect the conversation before sending again.")) }
    }
    private func connectionLost(_ reason: String) {
        detach(); connectionStatus = "Connection lost · Pi continues on your Mac"; error = reason + " Check the Mac bridge, Tailscale, pairing secret, and whether another phone is connected."
        guard foreground, wantsConnection else { return }
        let delay = retryDelay; retryDelay = min(retryDelay * 2, 30)
        retryTask = Task { [weak self] in try? await Task.sleep(nanoseconds: delay * 1_000_000_000); guard !Task.isCancelled else { return }; self?.connect() }
    }
    private func receive(_ incoming: IncomingRecord) throws {
        let record: WireEnvelope
        switch incoming {
        case .snapshot(let state, let rows): try apply(state, rows: rows); return
        case .envelope(let envelope): record = envelope
        }
        switch record.type {
        case "receipt":
            guard let id = record.id, let continuation = pending.removeValue(forKey: id) else { return }
            timeouts.removeValue(forKey: id)?.cancel()
            if record.ok == true { continuation.resume(returning: record.data ?? .null) } else { continuation.resume(throwing: MobileError(record.error ?? "The Mac rejected this action.")) }
        case "editor": if let id = record.id, !seenEditorIds.contains(id) { seenEditorIds.insert(id); editorOffer = record.text }
        default: throw MobileError("Unsupported bridge response. Update the app and bridge together.")
        }
    }
    private func apply(_ state: Snapshot, rows: [TranscriptRow]? = nil) throws {
        guard state.version == 1 else { throw MobileError("Unsupported bridge version. Update the app and bridge together.") }
        guard cursor.accept(epoch: state.epoch, revision: state.revision) else { return }
        let previous = snapshot?.sessionId
        let localDraft = draft
        let localAttachments = attachments
        let oldKey = draftKey
        if previous != state.sessionId { saveDraft() }
        if let rows { projectingSnapshot = true; transcriptRows = rows }
        snapshot = state
        projectingSnapshot = false
        let previousCanceled = seenCanceledIds.count
        for canceled in state.canceled ?? [] where !seenCanceledIds.contains(canceled.id) {
            submissions.append(Submission(id: canceled.id, text: canceled.text, sessionId: canceled.sessionId, status: "Canceled by Stop", recoverable: true))
            seenCanceledIds.insert(canceled.id)
        }
        if seenCanceledIds.count > 1000 { seenCanceledIds = Set(seenCanceledIds.suffix(1000)) }
        if previousCanceled != seenCanceledIds.count {
            let ids = Array(seenCanceledIds)
            writer.schedule(key: "canceled") { UserDefaults.standard.set(ids, forKey: "pi.canceledIds") }
        }
        if let offered = state.editor, !seenEditorIds.contains(offered.id) { seenEditorIds.insert(offered.id); editorOffer = offered.text }
        if previous != state.sessionId {
            var next = DraftState(text: drafts[draftKey] ?? "")
            if previous == nil && !localDraft.isEmpty && next.text != localDraft { next.restore(localDraft); drafts.removeValue(forKey: oldKey) }
            draft = next.text
            var nextAttachments = attachmentDrafts[draftKey] ?? []
            if previous == nil { for file in localAttachments where !nextAttachments.contains(where: { $0.id == file.id }) { nextAttachments.append(file) } }
            attachments = nextAttachments
        }
        if let failure = state.error { error = failure }
    }
    private func syncState() async throws {
        let current = generation
        let data = try await call("sync")
        let (state, rows) = try await Task.detached(priority: .userInitiated) {
            let state = try data.decode(Snapshot.self)
            return (state, TranscriptRows.make(messages: state.messages, tools: state.tools))
        }.value
        guard current == generation else { throw MobileError("Connection changed while refreshing. Reconnect before continuing.") }
        try apply(state, rows: rows)
    }
    private func call(_ op: String, fields: [String: Any] = [:], id: String = UUID().uuidString) async throws -> JSONValue {
        guard let socket else { throw MobileError("Connect to your Mac before sending.") }
        var record = fields; record["version"] = 1; record["id"] = id; record["op"] = op
        let data = try JSONSerialization.data(withJSONObject: record)
        guard let text = String(data: data, encoding: .utf8) else { throw MobileError("Could not encode the message.") }
        return try await withCheckedThrowingContinuation { continuation in
            pending[id] = continuation
            timeouts[id] = Task { [weak self] in
                try? await Task.sleep(nanoseconds: 35_000_000_000)
                guard !Task.isCancelled, let self, let continuation = self.pending.removeValue(forKey: id) else { return }
                self.timeouts.removeValue(forKey: id); continuation.resume(throwing: MobileError("No acknowledgement from your Mac. Delivery is uncertain; inspect history before sending again."))
            }
            Task { [weak self] in
                do { try await socket.send(.string(text)) }
                catch { guard let self, let continuation = self.pending.removeValue(forKey: id) else { return }; self.timeouts.removeValue(forKey: id)?.cancel(); continuation.resume(throwing: error) }
            }
        }
    }
    func submit(mode: String) {
        guard canSend, let state = snapshot else { return }
        let text = draft
        let files = attachments
        let content: ComposerPayload
        do { content = try ComposerPayload.make(text: text, attachments: files) } catch { self.error = error.localizedDescription; return }
        guard content.images.isEmpty || demo || state.capabilities?.contains("images") == true else { error = "Restart the updated Mac bridge before sending images."; return }
        let id = UUID().uuidString
        submissions.append(Submission(id: id, text: text, sessionId: state.sessionId, status: "Sending", recoverable: false, attachments: files)); draft = ""; attachments = []
        Task {
            // Persist the recoverable submission before transmission, without blocking typing.
            await withCheckedContinuation { continuation in writer.flush { continuation.resume() } }
            do {
                if demo { await demoReply(content.message); updateSubmission(id, status: "Accepted", recoverable: false); return }
                let data = try await call("send", fields: ["epoch": state.epoch, "text": content.message, "mode": mode, "images": content.images], id: id)
                let disposition = data["disposition"].text ?? "accepted"
                updateSubmission(id, status: disposition == "queued" ? "Queued" : disposition == "handled" ? "Handled by extension" : "Accepted", recoverable: false)
            } catch { updateSubmission(id, status: error.localizedDescription, recoverable: true); self.error = error.localizedDescription }
        }
    }
    private func updateSubmission(_ id: String, status: String, recoverable: Bool) { if let i = submissions.firstIndex(where: { $0.id == id }) { submissions[i].status = status; submissions[i].recoverable = recoverable } }
    func restore(_ submission: Submission) {
        let files = (submission.attachments ?? []).filter { file in !attachments.contains(where: { $0.id == file.id }) }
        guard attachments.count + files.count <= 4, (attachments + files).reduce(0, { $0 + $1.data.count }) <= 512 * 1024 else { error = "Remove a draft attachment before restoring these files."; return }
        var state = DraftState(text: draft); state.restore(submission.text); draft = state.text; attachments += files; submissions.removeAll { $0.id == submission.id }
    }
    func dismissSubmission(_ id: String) { submissions.removeAll { $0.id == id } }
    func useEditorOffer() { guard let offer = editorOffer else { return }; var state = DraftState(text: draft); state.restore(offer); draft = state.text; editorOffer = nil }
    func stop() async -> Bool {
        guard let state = snapshot, let run = state.runId else { return !busy }
        do {
            _ = try await call("stop", fields: ["epoch": state.epoch, "runId": run])
            try await syncState(); return true
        } catch { self.error = error.localizedDescription; return false }
    }
    func loadConversations() async {
        if demo { conversations = [Conversation(id: "previous", title: "A previous thought", date: nil)]; return }
        do { let value = try await call("sessions")["sessions"]; conversations = try await Task.detached(priority: .userInitiated) { try value.decode([Conversation].self) }.value } catch { self.error = error.localizedDescription }
    }
    func browse(_ conversation: Conversation) async {
        if demo { browsing = History(sessionId: conversation.id, title: conversation.title, messages: [ChatMessage(id: "old", role: "assistant", text: "There is room here for a fresh idea.")]); return }
        do { let value = try await call("history", fields: ["sessionId": conversation.id]); browsing = try await Task.detached(priority: .userInitiated) { try value.decode(History.self) }.value } catch { self.error = error.localizedDescription }
    }
    func changeSession(to id: String? = nil, stopFirst: Bool = false) async {
        guard !changingSession, let state = snapshot else { return }; changingSession = true; defer { changingSession = false }
        if busy { guard stopFirst, await stop() else { return } }
        do {
            var fields: [String: Any] = ["epoch": state.epoch]; if let id { fields["sessionId"] = id }
            _ = try await call(id == nil ? "new" : "resume", fields: fields)
            try await syncState(); browsing = nil
        } catch { self.error = error.localizedDescription }
    }
    func answer(_ dialog: ExtensionDialog, value: String? = nil, confirmed: Bool? = nil, cancelled: Bool = false) async {
        var fields: [String: Any] = ["dialogId": dialog.id, "cancelled": cancelled]; if let value { fields["value"] = value }; if let confirmed { fields["confirmed"] = confirmed }
        do { _ = try await call("answer", fields: fields) } catch { self.error = error.localizedDescription }
    }
    func exploreDemo() {
        disconnect(); demo = true; connected = true; connectionStatus = "On-device preview"; error = nil; cursor.reset()
        snapshot = Snapshot(version: 1, epoch: "preview", revision: 1, busy: false, sessionId: "preview", title: "PurePoint", messages: [ChatMessage(id: "welcome", role: "assistant", text: "What would you like to work on?")], tools: [], queue: [], dialogs: [], notices: [])
    }
    private func demoReply(_ text: String) async {
        guard let old = snapshot else { return }
        let demoGeneration = generation
        let messages = old.messages + [ChatMessage(id: UUID().uuidString, role: "user", text: text)]
        snapshot = Snapshot(version: 1, epoch: old.epoch, revision: old.revision + 1, busy: true, runId: "preview-run", sessionId: old.sessionId, title: old.title, messages: messages, tools: [ToolActivity(id: "demo-tool", name: "read", state: "running", text: "Reading the project notes…")], queue: [], dialogs: [], notices: [])
        try? await Task.sleep(nanoseconds: 800_000_000)
        guard demo, generation == demoGeneration else { return }
        snapshot = Snapshot(version: 1, epoch: old.epoch, revision: old.revision + 2, busy: false, sessionId: old.sessionId, title: old.title, messages: messages + [ChatMessage(id: UUID().uuidString, role: "assistant", text: "I’ve read the project notes. Here’s the next step.\n\n```swift\nlet nextStep = \"Build something useful\"\n```")], tools: [ToolActivity(id: "demo-tool", name: "read", state: "finished", text: "Project notes read.")], queue: [], dialogs: [], notices: [])
    }
}
