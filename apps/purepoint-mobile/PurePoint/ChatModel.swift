import Foundation
import SwiftUI
import Network

// UserDefaults is thread-safe; the immutable reference is shared with the utility writer.
private struct RecoveryPreferences: @unchecked Sendable { let value: UserDefaults }

@MainActor final class ChatModel: ObservableObject {
    @Published var snapshot: Snapshot? { didSet { if !projectingSnapshot, let snapshot { transcriptRows = TranscriptRows.make(messages: snapshot.messages, tools: snapshot.tools) } else if snapshot == nil { transcriptRows = [] } } }
    private(set) var transcriptRows: [TranscriptRow] = []
    @Published var connected = false
    @Published var connectionStatus = "Not connected"
    @Published var error: String?
    @Published var draft = "" { didSet { saveDraft() } }
    @Published var attachments: [ComposerAttachment] = [] { didSet { saveAttachmentDraft() } }
    @Published var submissions: [Submission] = [] {
        didSet {
            if !normalizingSubmissions {
                if submissions.filter({ $0.recoverable || $0.status == "Sending" }).count > 50 {
                    error = "Message recovery is full. Older recovery receipts could not be retained; inspect native history before sending again. Restore or dismiss saved recovery to make room."
                }
                normalizingSubmissions = true
                submissions = Self.retainedSubmissions(submissions)
                normalizingSubmissions = false
                saveSubmissions()
            }
        }
    }
    @Published var conversations: [Conversation] = []
    @Published var browsing: History?
    @Published var editorOffer: String?
    @Published var changingSession = false
    @Published var endpoint = ""
    private var socket: URLSessionWebSocketTask?
    private var receiveTask: Task<Void, Never>?
    private var retryTask: Task<Void, Never>?
    private var pending: [String: CheckedContinuation<JSONValue, Error>] = [:]
    private var timeouts: [String: Task<Void, Never>] = [:]
    private var drafts: [String: String] = [:]
    private var attachmentDrafts: [String: [ComposerAttachment]] = [:]
    private let writer = CoalescedWriter()
    private var recoveryTask: Task<Void, Never>?
    private var connectionTask: Task<Void, Never>?
    private var recoveryLoaded = false
    private var normalizingSubmissions = false
    private var projectingSnapshot = false
    private var cursor = SnapshotCursor()
    private var seenEditorIds = Set<String>()
    private var canceledIdHistory: [String] = []
    private var seenCanceledIds = Set<String>()
    private var generation = 0
    private var foreground = true
    private var wantsConnection = false
    private var retryBudget = ReconnectBudget()
    private var hostSession: URLSession?
    private var hostPin: PinnedHostSession?
    private var pairingTask: Task<Void, Never>?
    private var pairingGeneration = 0
    private let networkMonitor = NWPathMonitor()
    @Published private(set) var pairing = false
    var demo = false
    var busy: Bool { snapshot?.busy ?? false }
    var canSend: Bool { connected && snapshot?.error == nil && (!demo || !busy) && !changingSession && (!busy || attachments.isEmpty) && (!draft.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || !attachments.isEmpty) }
    var draftKey: String { endpoint + ":" + (snapshot?.sessionId ?? "local") }
    var recoverable: [Submission] { submissions.filter { $0.recoverable } }

    private(set) var clientId: String

    private let trustWriter: PairingTrustWriter
    private let trustLoader: @Sendable (String) throws -> TrustedHost?
    private let defaults: RecoveryPreferences

    init(defaults: UserDefaults = .standard, trustLoader: @escaping @Sendable (String) throws -> TrustedHost? = { try PairingSecret.readTrust(endpoint: $0) }, trustSaver: @escaping @Sendable (TrustedHost) throws -> Void = { try PairingSecret.saveTrust($0) }) {
        self.trustWriter = PairingTrustWriter(save: trustSaver)
        self.trustLoader = trustLoader
        let defaults = RecoveryPreferences(value: defaults)
        self.defaults = defaults
        clientId = defaults.value.string(forKey: "pi.clientId") ?? UUID().uuidString
        defaults.value.set(clientId, forKey: "pi.clientId")
        endpoint = defaults.value.string(forKey: "pi.endpoint") ?? ""
        drafts = defaults.value.dictionary(forKey: "pi.drafts") as? [String: String] ?? [:]
        let savedCanceledIds = defaults.value.stringArray(forKey: "pi.canceledIds") ?? []
        canceledIdHistory = Self.recentCanceledIds(savedCanceledIds)
        seenCanceledIds = Set(canceledIdHistory)
        draft = drafts[draftKey] ?? ""
        if canceledIdHistory != savedCanceledIds { saveCanceledIds() }
        networkMonitor.pathUpdateHandler = { [weak self] path in
            let reachable = path.status == .satisfied
            Task { @MainActor [weak self] in self?.networkChanged(reachable: reachable) }
        }
        networkMonitor.start(queue: DispatchQueue(label: "pointguard.reachability"))
        recoveryTask = Task { [weak self] in
            let saved = await Task.detached(priority: .utility) {
                return (LocalRecoveryStore.load("attachment-drafts", as: [String: [ComposerAttachment]].self) ?? [:], LocalRecoveryStore.load("submissions", as: [Submission].self) ?? [])
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
    deinit { networkMonitor.cancel() }
    private static func recentCanceledIds(_ ids: [String]) -> [String] {
        var seen = Set<String>()
        return Array(ids.reversed().filter { seen.insert($0).inserted }.prefix(1000).reversed())
    }
    private func saveCanceledIds() {
        let ids = canceledIdHistory
        let defaults = defaults
        writer.schedule(key: "canceled") { defaults.value.set(ids, forKey: "pi.canceledIds") }
    }
    private func saveDraft() {
        drafts[draftKey] = String(draft.prefix(65536))
        if drafts.count > 100 { drafts.removeValue(forKey: drafts.keys.first(where: { $0 != draftKey }) ?? "") }
        let value = drafts
        let defaults = defaults
        writer.schedule(key: "drafts") { defaults.value.set(value, forKey: "pi.drafts") }
    }
    private static func retainedSubmissions(_ items: [Submission]) -> [Submission] {
        // In-flight sends outrank recovery, which outranks settled receipts. Within
        // each priority retain the newest, then preserve chronological display order.
        func priority(_ item: Submission) -> Int { item.status == "Sending" ? 2 : item.recoverable ? 1 : 0 }
        let kept = Set(items.indices.sorted {
            let left = priority(items[$0]), right = priority(items[$1])
            return left == right ? $0 > $1 : left > right
        }.prefix(50))
        return items.enumerated().compactMap { index, item in
            guard kept.contains(index) else { return nil }
            var item = item
            if !item.recoverable && item.status != "Sending" { item.attachments = nil }
            return item
        }
    }
    private func saveSubmissions() {
        guard recoveryLoaded else { return }
        let value = submissions
        writer.schedule(key: "submissions") { [weak self] in
            do { try LocalRecoveryStore.save(value, name: "submissions") }
            catch { Task { @MainActor [weak self] in
                if self?.error?.hasPrefix("Message not sent:") != true { self?.error = "Could not save message recovery on this phone." }
            } }
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
    func pair(_ code: PairingCode) {
        guard PairingCode.parse((try? String(data: JSONEncoder().encode(PairingWire(code)), encoding: .utf8)) ?? "") != nil else { error = "This QR expired. Create a new code on your Mac."; return }
        pairingTask?.cancel(); pairingGeneration += 1; trustWriter.advance(to: pairingGeneration); let attempt = pairingGeneration
        pairing = true; error = nil
        pairingTask = Task { [weak self] in
            let pin = PinnedHostSession(endpoint: code.endpoint, certificateSHA256: code.certificateSHA256)
            let session = pin.makeSession(); defer { session.invalidateAndCancel() }
            do {
                var request = URLRequest(url: Self.httpURL(code.endpoint, path: "/pair/enroll"))
                request.httpMethod = "POST"; request.setValue("application/json", forHTTPHeaderField: "Content-Type")
                request.httpBody = try JSONSerialization.data(withJSONObject: ["version": 1, "enrollmentToken": code.enrollmentToken, "name": "iPhone"])
                let (data, response) = try await session.data(for: request)
                guard (response as? HTTPURLResponse)?.statusCode == 200 else { throw TrustFailure("Enrollment expired, used or revoked. Create a new Mac QR code.") }
                let device = try JSONDecoder().decode(EnrolledDevice.self, from: data)
                guard device.version == 1, device.hostId == code.hostId else { throw TrustFailure("Mac identity changed. Scan a new QR code deliberately.") }
                let saved = TrustedHost(version: 1, endpoint: code.endpoint, hostId: code.hostId, certificateSHA256: code.certificateSHA256, deviceId: device.deviceId, clientId: device.clientId, credential: device.credential)
                guard let self, !Task.isCancelled, attempt == self.pairingGeneration else { return }
                try await self.trustWriter.commit(saved, generation: attempt)
                guard !Task.isCancelled, attempt == self.pairingGeneration else { return }
                self.saveDraft(); self.saveAttachmentDraft(); self.disconnect(cancelPairing: false)
                self.endpoint = saved.endpoint; self.defaults.value.set(saved.endpoint, forKey: "pi.endpoint")
                self.snapshot = nil; self.draft = self.drafts[self.draftKey] ?? ""; self.attachments = self.attachmentDrafts[self.draftKey] ?? []
                self.demo = false; self.pairing = false; self.pairingTask = nil; self.connect()
            } catch {
                guard let self, attempt == self.pairingGeneration else { return }
                self.pairing = false; self.pairingTask = nil
                self.error = pin.identityRejected ? "Mac certificate does not match this QR. Scan a new code deliberately." : "Pairing failed. " + error.localizedDescription + " Create a new Mac QR code; enrollment is never replayed."
            }
        }
    }
    private struct PairingWire: Encodable {
        let type: String; let version: Int; let endpoint: String; let hostId: String; let certificateSHA256: String; let enrollmentToken: String; let expiresAt: Int64
        init(_ code: PairingCode) { type = code.type; version = code.version; endpoint = code.endpoint; hostId = code.hostId; certificateSHA256 = code.certificateSHA256; enrollmentToken = code.enrollmentToken; expiresAt = code.expiresAt }
    }
    private struct EnrolledDevice: Decodable { let version: Int; let hostId: String; let deviceId: String; let clientId: String; let credential: String }
    private struct VerifiedDevice: Decodable { let version: Int; let hostId: String; let deviceId: String; let clientId: String }
    private static func httpURL(_ endpoint: String, path: String) -> URL {
        var components = URLComponents(string: endpoint)!
        components.scheme = "https"; components.path = path
        return components.url!
    }
    func connect(resetRetryBudget: Bool = true) {
        guard !demo, foreground, socket == nil, connectionTask == nil else { return }
        guard let url = ConnectionAddress.url(endpoint), url.scheme == "wss" else {
            if !endpoint.isEmpty { requirePairing("Legacy or invalid Mac trust. Scan a new Mac QR code.") }
            return
        }
        if resetRetryBudget { retryBudget.reset() }
        wantsConnection = true; retryTask?.cancel()
        let address = endpoint; generation += 1; let current = generation
        connectionStatus = "Connecting to your Mac…"
        connectionTask = Task { [weak self] in
            await self?.recoveryTask?.value
            var session: URLSession?; var pin: PinnedHostSession?
            do {
                guard let saved = try await Task.detached(priority: .userInitiated, operation: { try self?.trustLoader(address) }).value else { throw TrustFailure("No saved device trust. Scan a new Mac QR code.") }
                guard let self, !Task.isCancelled, current == self.generation else { return }
                let delegate = PinnedHostSession(endpoint: address, certificateSHA256: saved.certificateSHA256)
                let connection = delegate.makeSession(); session = connection; pin = delegate
                var verify = URLRequest(url: Self.httpURL(address, path: "/pair/verify"))
                verify.setValue("Bearer " + saved.credential, forHTTPHeaderField: "Authorization")
                let (data, response) = try await connection.data(for: verify)
                let status = (response as? HTTPURLResponse)?.statusCode
                if status == 401 || status == 403 { throw TrustFailure("This phone’s trust was revoked or lost. Scan a new Mac QR code deliberately.") }
                guard status == 200 else { throw MobileError("Mac trust verification is unavailable.") }
                let verified = try JSONDecoder().decode(VerifiedDevice.self, from: data)
                guard verified.version == 1, verified.hostId == saved.hostId, verified.deviceId == saved.deviceId, verified.clientId == saved.clientId else { throw TrustFailure("Saved Mac identity changed. Scan a new QR code deliberately.") }
                guard !Task.isCancelled, current == self.generation, self.foreground, self.wantsConnection, self.endpoint == address else { connection.invalidateAndCancel(); return }
                self.connectionTask = nil; self.clientId = saved.clientId; self.hostSession = connection; self.hostPin = delegate
                self.startConnection(url: url, credential: saved.credential, session: connection)
            } catch {
                session?.invalidateAndCancel()
                guard let self, !Task.isCancelled, current == self.generation else { return }
                self.connectionTask = nil
                if pin?.identityRejected == true || error is TrustFailure { self.requirePairing(pin?.identityRejected == true ? "Mac certificate changed. Scan a new QR code deliberately." : error.localizedDescription) }
                else { self.connectionLost(error.localizedDescription) }
            }
        }
    }
    private func startConnection(url: URL, credential: String, session: URLSession) {
        generation += 1; let current = generation
        cursor.reset(); connectionStatus = "Connecting to your Mac…"; error = nil
        var request = URLRequest(url: url); request.setValue("Bearer " + credential, forHTTPHeaderField: "Authorization")
        request.setValue(clientId, forHTTPHeaderField: "X-PointGuard-Client-ID")
        let task = session.webSocketTask(with: request); task.maximumMessageSize = 4 * 1024 * 1024; socket = task; task.resume()
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
            } catch {
                guard let self, current == self.generation else { return }
                if task.closeCode.rawValue == 4001 || self.hostPin?.identityRejected == true { self.requirePairing("Mac trust changed or this phone was revoked. Scan a new QR code deliberately.") }
                else { self.connectionLost(error.localizedDescription) }
            }
        }
        Task { [weak self] in
            guard let self else { return }
            do {
                try await self.syncState()
                guard current == self.generation else { return }
                self.connected = true; self.connectionStatus = "Connected to your Mac"; self.retryBudget.reset()
            } catch { if current == self.generation { self.connectionLost(error.localizedDescription) } }
        }
    }
    func setForeground(_ active: Bool) {
        foreground = active
        if active { retryBudget.reset(); if wantsConnection { connect(resetRetryBudget: false) } }
        else {
            writer.flush(); retryTask?.cancel(); pairingTask?.cancel(); pairingGeneration += 1; trustWriter.advance(to: pairingGeneration); pairing = false; pairingTask = nil
            detach(); connectionStatus = "Paused on this phone · Pi continues on your Mac"
        }
    }
    private func networkChanged(reachable: Bool) {
        guard foreground, wantsConnection else { return }
        if reachable { retryBudget.reset(); if socket == nil { connect(resetRetryBudget: false) } }
        else { retryTask?.cancel(); detach(); connectionStatus = "Waiting for network · Pi continues on your Mac" }
    }
    func disconnect(cancelPairing: Bool = true) {
        if cancelPairing {
            pairingTask?.cancel(); pairingGeneration += 1; trustWriter.advance(to: pairingGeneration); pairingTask = nil; pairing = false
        }
        wantsConnection = false; retryTask?.cancel(); detach(); connectionStatus = "Not connected"
    }
    private func detach() {
        generation += 1; connectionTask?.cancel(); connectionTask = nil; connected = false; receiveTask?.cancel(); receiveTask = nil; socket?.cancel(with: .goingAway, reason: nil); socket = nil
        hostSession?.invalidateAndCancel(); hostSession = nil; hostPin = nil
        let outstanding = pending; pending = [:]; for timeout in timeouts.values { timeout.cancel() }; timeouts = [:]
        for continuation in outstanding.values { continuation.resume(throwing: MobileError("Connection interrupted. Delivery may be uncertain. Inspect the conversation before sending again.")) }
    }
    private func requirePairing(_ reason: String) {
        wantsConnection = false; retryTask?.cancel(); detach(); connectionStatus = "Re-pair with your Mac"; error = reason
    }
    private func connectionLost(_ reason: String) {
        retryTask?.cancel(); detach(); connectionStatus = "Connection lost · Pi continues on your Mac"; error = reason + " Check PurePoint on your Mac and Tailscale."
        guard foreground, wantsConnection else { return }
        guard let delay = retryBudget.nextDelay() else { connectionStatus = "Reconnect paused · Check your Mac and network"; return }
        retryTask = Task { [weak self] in try? await Task.sleep(nanoseconds: delay * 1_000_000_000); guard !Task.isCancelled else { return }; self?.connect(resetRetryBudget: false) }
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
        let previousCanceled = canceledIdHistory
        for canceled in state.canceled ?? [] where canceled.clientId == clientId && !seenCanceledIds.contains(canceled.id) {
            submissions.append(Submission(id: canceled.id, text: canceled.text, sessionId: canceled.sessionId, status: "Canceled by Stop", recoverable: true))
            seenCanceledIds.insert(canceled.id)
        }
        // Refresh the current snapshot's IDs in wire order so older history
        // cannot evict them while the bridge continues including them.
        canceledIdHistory = Self.recentCanceledIds(canceledIdHistory + (state.canceled ?? []).filter { $0.clientId == clientId }.map(\.id))
        seenCanceledIds = Set(canceledIdHistory)
        if previousCanceled != canceledIdHistory { saveCanceledIds() }
        if let offered = state.editor, !seenEditorIds.contains(offered.id) { seenEditorIds.insert(offered.id); editorOffer = offered.text }
        if previous != state.sessionId {
            var next = DraftState(text: drafts[draftKey] ?? "")
            if previous == nil && !localDraft.isEmpty && next.text != localDraft { next.restore(localDraft); drafts.removeValue(forKey: oldKey) }
            draft = next.text
            var nextAttachments = attachmentDrafts[draftKey] ?? []
            if previous == nil {
                for file in localAttachments where !nextAttachments.contains(where: { $0.id == file.id }) { nextAttachments.append(file) }
                attachmentDrafts.removeValue(forKey: oldKey)
            }
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
        var record = fields; record["version"] = 1
        record["clientId"] = clientId; record["id"] = id; record["op"] = op
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
        guard submissions.filter({ $0.recoverable || $0.status == "Sending" }).count < 50 else {
            error = "Message recovery is full. Restore or dismiss a saved recovery before sending another message."
            return
        }
        let text = draft
        let files = attachments
        let content: ComposerPayload
        do { content = try ComposerPayload.make(text: text, attachments: files) } catch { self.error = error.localizedDescription; return }
        guard content.images.isEmpty || demo || state.capabilities?.contains("images") == true else { error = "Restart the updated Mac bridge before sending images."; return }
        let id = UUID().uuidString
        submissions.append(Submission(id: id, text: text, sessionId: state.sessionId, status: "Sending", recoverable: false, attachments: files)); draft = ""; attachments = []
        Task {
            // Hydration and confirmed persistence precede any transmission, while
            // the composer remains editable. Sending receipts cannot be dismissed.
            await recoveryTask?.value
            let receipts = submissions
            do {
                try await writer.writeAndConfirm { try LocalRecoveryStore.save(receipts, name: "submissions") }
            } catch {
                let guidance = "Message not sent: could not save recovery on this phone. Free storage, then restore the saved message and try again. Keep the app open to retain its text and files."
                updateSubmission(id, status: guidance, recoverable: true); self.error = guidance
                return
            }
            do {
                if demo { await demoReply(content.message); updateSubmission(id, status: "Accepted", recoverable: false); return }
                let data = try await call("send", fields: ["epoch": state.epoch, "text": content.message, "mode": mode, "images": content.images], id: id)
                let disposition = data["disposition"].text ?? "accepted"
                updateSubmission(id, status: disposition == "queued" ? "Queued" : disposition == "handled" ? "Handled by extension" : "Accepted", recoverable: false)
            } catch { updateSubmission(id, status: error.localizedDescription, recoverable: true); self.error = error.localizedDescription }
        }
    }
    private func updateSubmission(_ id: String, status: String, recoverable: Bool) {
        if let i = submissions.firstIndex(where: { $0.id == id }) {
            var item = submissions[i]; item.status = status; item.recoverable = recoverable
            submissions[i] = item
        }
    }
    func restore(_ submission: Submission) {
        guard submissions.first(where: { $0.id == submission.id })?.status != "Sending" else { error = "Wait for this message's delivery result before restoring it."; return }
        let files = (submission.attachments ?? []).filter { file in !attachments.contains(where: { $0.id == file.id }) }
        guard attachments.count + files.count <= 4, (attachments + files).reduce(0, { $0 + $1.data.count }) <= 512 * 1024 else { error = "Remove a draft attachment before restoring these files."; return }
        var state = DraftState(text: draft); state.restore(submission.text); draft = state.text; attachments += files; submissions.removeAll { $0.id == submission.id }
    }
    func dismissSubmission(_ id: String) {
        guard submissions.first(where: { $0.id == id })?.status != "Sending" else { error = "Wait for this message's delivery result before dismissing it."; return }
        submissions.removeAll { $0.id == id }
    }
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
    func answer(_ dialog: ExtensionDialog, value: String? = nil, optionId: String? = nil, confirmed: Bool? = nil, cancelled: Bool = false) async {
        guard let state = snapshot else { return }
        var fields: [String: Any] = ["epoch": state.epoch, "dialogId": dialog.id, "cancelled": cancelled]; if let optionId { fields["optionId"] = optionId }; if let value { fields["value"] = value }; if let confirmed { fields["confirmed"] = confirmed }
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
