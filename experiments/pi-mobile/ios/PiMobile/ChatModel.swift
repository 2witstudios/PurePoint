import Foundation
import SwiftUI

@MainActor final class ChatModel: ObservableObject {
    @Published var snapshot: Snapshot?
    @Published var connected = false
    @Published var connectionStatus = "Not connected"
    @Published var error: String?
    @Published var draft = "" { didSet { saveDraft() } }
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
    private var cursor = SnapshotCursor()
    private var seenEditorIds = Set<String>()
    private var seenCanceledIds = Set(UserDefaults.standard.stringArray(forKey: "pi.canceledIds") ?? [])
    private var generation = 0
    private var foreground = true
    private var wantsConnection = false
    private var retryDelay: UInt64 = 2
    var demo = false
    var busy: Bool { snapshot?.busy ?? false }
    var canSend: Bool { connected && snapshot?.error == nil && (!demo || !busy) && !changingSession && !draft.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty }
    var draftKey: String { endpoint + ":" + (snapshot?.sessionId ?? "local") }
    var recoverable: [Submission] { submissions.filter { $0.recoverable } }

    init() {
        if let data = UserDefaults.standard.data(forKey: "pi.submissions"), let saved = try? JSONDecoder().decode([Submission].self, from: data) {
            submissions = saved.map { item in var item = item; if item.status == "Sending" { item.status = "Delivery uncertain"; item.recoverable = true }; return item }
        }
        draft = drafts[draftKey] ?? ""
    }
    private func saveDraft() { drafts[draftKey] = String(draft.prefix(65536)); if drafts.count > 100 { drafts.removeValue(forKey: drafts.keys.first ?? "") }; UserDefaults.standard.set(drafts, forKey: "pi.drafts") }
    private func saveSubmissions() { if let data = try? JSONEncoder().encode(Array(submissions.suffix(50))) { UserDefaults.standard.set(data, forKey: "pi.submissions") } }
    func pair(endpoint: String, secret: String) {
        guard let url = ConnectionAddress.url(endpoint) else { error = "Enter a Tailscale address ending in /v1, such as ws://100.100.1.2:8787/v1."; return }
        guard secret.count >= 32 else { error = "Enter the pairing secret from your Mac (at least 32 characters)."; return }
        do { try PairingSecret.save(secret, endpoint: url.absoluteString) } catch { self.error = error.localizedDescription; return }
        saveDraft(); disconnect(); self.endpoint = url.absoluteString; UserDefaults.standard.set(self.endpoint, forKey: "pi.endpoint"); snapshot = nil; draft = drafts[draftKey] ?? ""; demo = false; wantsConnection = true; connect()
    }
    func connect() {
        guard !demo, foreground, socket == nil, let url = ConnectionAddress.url(endpoint) else { return }
        let secret = PairingSecret.read(endpoint: endpoint)
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
                    try self.receive(data)
                }
            } catch { guard let self, current == self.generation else { return }; self.connectionLost(error.localizedDescription) }
        }
        Task { [weak self] in
            guard let self else { return }
            do { let data = try await self.call("sync"); guard current == self.generation else { return }; let state = try data.decode(Snapshot.self); try self.apply(state); self.connected = true; self.connectionStatus = "Connected to your Mac"; self.retryDelay = 2 }
            catch { if current == self.generation { self.connectionLost(error.localizedDescription) } }
        }
    }
    func setForeground(_ active: Bool) { foreground = active; if active { if wantsConnection || !endpoint.isEmpty { connect() } } else { detach(); connectionStatus = "Paused on this phone · Pi continues on your Mac" } }
    func disconnect() { wantsConnection = false; retryTask?.cancel(); detach(); connectionStatus = "Not connected" }
    private func detach() {
        generation += 1; connected = false; receiveTask?.cancel(); receiveTask = nil; socket?.cancel(with: .goingAway, reason: nil); socket = nil
        let outstanding = pending; pending = [:]; for timeout in timeouts.values { timeout.cancel() }; timeouts = [:]
        for continuation in outstanding.values { continuation.resume(throwing: MobileError("Connection interrupted. Delivery may be uncertain. Inspect the conversation before sending again.")) }
    }
    private func connectionLost(_ reason: String) {
        detach(); connectionStatus = "Connection lost · Pi continues on your Mac"; error = reason + " Check the Mac bridge, Tailscale, pairing secret, and whether another phone is connected."
        guard foreground, wantsConnection else { return }
        let delay = retryDelay; retryDelay = min(retryDelay * 2, 30)
        retryTask = Task { [weak self] in try? await Task.sleep(nanoseconds: delay * 1_000_000_000); guard !Task.isCancelled else { return }; self?.connect() }
    }
    private func receive(_ data: Data) throws {
        let record = try JSONDecoder().decode(WireEnvelope.self, from: data)
        switch record.type {
        case "snapshot": try apply(JSONDecoder().decode(Snapshot.self, from: data))
        case "receipt":
            guard let id = record.id, let continuation = pending.removeValue(forKey: id) else { return }
            timeouts.removeValue(forKey: id)?.cancel()
            if record.ok == true { continuation.resume(returning: record.data ?? .null) } else { continuation.resume(throwing: MobileError(record.error ?? "The Mac rejected this action.")) }
        case "editor": if let id = record.id, !seenEditorIds.contains(id) { seenEditorIds.insert(id); editorOffer = record.text }
        default: throw MobileError("Unsupported bridge response. Update the app and bridge together.")
        }
    }
    private func apply(_ state: Snapshot) throws {
        guard state.version == 1 else { throw MobileError("Unsupported bridge version. Update the app and bridge together.") }
        guard cursor.accept(epoch: state.epoch, revision: state.revision) else { return }
        let previous = snapshot?.sessionId
        let localDraft = draft
        let oldKey = draftKey
        if previous != state.sessionId { saveDraft() }
        snapshot = state
        for canceled in state.canceled ?? [] where !seenCanceledIds.contains(canceled.id) {
            submissions.append(Submission(id: canceled.id, text: canceled.text, sessionId: canceled.sessionId, status: "Canceled by Stop", recoverable: true))
            seenCanceledIds.insert(canceled.id)
        }
        if seenCanceledIds.count > 1000 { seenCanceledIds = Set(seenCanceledIds.suffix(1000)) }
        UserDefaults.standard.set(Array(seenCanceledIds), forKey: "pi.canceledIds")
        if let offered = state.editor, !seenEditorIds.contains(offered.id) { seenEditorIds.insert(offered.id); editorOffer = offered.text }
        if previous != state.sessionId {
            var next = DraftState(text: drafts[draftKey] ?? "")
            if previous == nil && !localDraft.isEmpty && next.text != localDraft { next.restore(localDraft); drafts.removeValue(forKey: oldKey) }
            draft = next.text
        }
        if let failure = state.error { error = failure }
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
        guard text.utf8.count <= 65536 else { error = "Message is too long. Keep it under 64 KiB."; return }
        let id = UUID().uuidString
        submissions.append(Submission(id: id, text: text, sessionId: state.sessionId, status: "Sending", recoverable: false)); draft = ""
        Task {
            do {
                if demo { await demoReply(text); updateSubmission(id, status: "Accepted", recoverable: false); return }
                let data = try await call("send", fields: ["epoch": state.epoch, "text": text, "mode": mode], id: id)
                let disposition = data["disposition"].text ?? "accepted"
                updateSubmission(id, status: disposition == "queued" ? "Queued" : disposition == "handled" ? "Handled by extension" : "Accepted", recoverable: false)
            } catch { updateSubmission(id, status: error.localizedDescription, recoverable: true); self.error = error.localizedDescription }
        }
    }
    private func updateSubmission(_ id: String, status: String, recoverable: Bool) { if let i = submissions.firstIndex(where: { $0.id == id }) { submissions[i].status = status; submissions[i].recoverable = recoverable } }
    func restore(_ submission: Submission) { var state = DraftState(text: draft); state.restore(submission.text); draft = state.text; submissions.removeAll { $0.id == submission.id } }
    func dismissSubmission(_ id: String) { submissions.removeAll { $0.id == id } }
    func useEditorOffer() { guard let offer = editorOffer else { return }; var state = DraftState(text: draft); state.restore(offer); draft = state.text; editorOffer = nil }
    func stop() async -> Bool {
        guard let state = snapshot, let run = state.runId else { return !busy }
        do {
            _ = try await call("stop", fields: ["epoch": state.epoch, "runId": run])
            let fresh = try await call("sync"); try apply(fresh.decode(Snapshot.self)); return true
        } catch { self.error = error.localizedDescription; return false }
    }
    func loadConversations() async {
        if demo { conversations = [Conversation(id: "previous", title: "A previous thought", date: nil)]; return }
        do { conversations = try await call("sessions")["sessions"].decode([Conversation].self) } catch { self.error = error.localizedDescription }
    }
    func browse(_ conversation: Conversation) async {
        if demo { browsing = History(sessionId: conversation.id, title: conversation.title, messages: [ChatMessage(id: "old", role: "assistant", text: "There is room here for a fresh idea.")]); return }
        do { browsing = try await call("history", fields: ["sessionId": conversation.id]).decode(History.self) } catch { self.error = error.localizedDescription }
    }
    func changeSession(to id: String? = nil, stopFirst: Bool = false) async {
        guard !changingSession, let state = snapshot else { return }; changingSession = true; defer { changingSession = false }
        if busy { guard stopFirst, await stop() else { return } }
        do {
            var fields: [String: Any] = ["epoch": state.epoch]; if let id { fields["sessionId"] = id }
            _ = try await call(id == nil ? "new" : "resume", fields: fields)
            let fresh = try await call("sync"); try apply(fresh.decode(Snapshot.self)); browsing = nil
        } catch { self.error = error.localizedDescription }
    }
    func answer(_ dialog: ExtensionDialog, value: String? = nil, confirmed: Bool? = nil, cancelled: Bool = false) async {
        var fields: [String: Any] = ["dialogId": dialog.id, "cancelled": cancelled]; if let value { fields["value"] = value }; if let confirmed { fields["confirmed"] = confirmed }
        do { _ = try await call("answer", fields: fields) } catch { self.error = error.localizedDescription }
    }
    func exploreDemo() {
        disconnect(); demo = true; connected = true; connectionStatus = "On-device preview"; error = nil; cursor.reset()
        snapshot = Snapshot(version: 1, epoch: "preview", revision: 1, busy: false, sessionId: "preview", title: "A little room to think", messages: [ChatMessage(id: "welcome", role: "assistant", text: "What would you like to work on?\n\nSend a complete thought. I’ll keep working on your Mac while you’re away.")], tools: [], queue: [], dialogs: [], notices: [])
    }
    private func demoReply(_ text: String) async {
        guard let old = snapshot else { return }
        let demoGeneration = generation
        let messages = old.messages + [ChatMessage(id: UUID().uuidString, role: "user", text: text)]
        snapshot = Snapshot(version: 1, epoch: old.epoch, revision: old.revision + 1, busy: true, runId: "preview-run", sessionId: old.sessionId, title: old.title, messages: messages, tools: [ToolActivity(id: "demo-tool", name: "read", state: "running", text: "Reading the project notes…")], queue: [], dialogs: [], notices: [])
        try? await Task.sleep(nanoseconds: 800_000_000)
        guard demo, generation == demoGeneration else { return }
        snapshot = Snapshot(version: 1, epoch: old.epoch, revision: old.revision + 2, busy: false, sessionId: old.sessionId, title: old.title, messages: messages + [ChatMessage(id: UUID().uuidString, role: "assistant", text: "Let’s make it concrete.\n\nThis preview exercises the native conversation layout. Connect to the fixture bridge for queue, Stop, reconnect, history and extension dialogs.\n\n```swift\nlet nextStep = \"Build something useful\"\n```")], tools: [ToolActivity(id: "demo-tool", name: "read", state: "finished", text: "Project notes read.")], queue: [], dialogs: [], notices: [])
    }
}
