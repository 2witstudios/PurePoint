import Foundation
import SwiftUI
import Network

// UserDefaults is thread-safe; the immutable reference is shared with the utility writer.
private struct RecoveryPreferences: @unchecked Sendable { let value: UserDefaults }

@MainActor final class ChatModel: ObservableObject {
    let agentControl = AgentControlModel()
    @Published var snapshot: Snapshot? { didSet { if !projectingSnapshot, let snapshot { transcriptRows = TranscriptRows.make(messages: snapshot.messages, tools: snapshot.tools) } else if snapshot == nil { transcriptRows = [] } } }
    private(set) var transcriptRows: [TranscriptRow] = []
    @Published var connected = false
    /// Mirrors the chat access the host derives from this phone's grants; the bridge enforces it.
    @Published private(set) var chatGrant: ChatGrant = .send
    @Published var connectionStatus = "Not connected"
    @Published var error: String?
    @Published var draft = "" { didSet { saveDraft() } }
    @Published var attachments: [ComposerAttachment] = [] { didSet { saveAttachmentDraft() } }
    @Published var submissions: [Submission] = [] {
        didSet {
            if !normalizingSubmissions {
                if submissions.filter({ $0.recoverable || $0.status == "Sending" || $0.status == "Queued" }).count > 50 {
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
    @Published private(set) var endpoint = ""
    @Published private(set) var registry = MachineRegistry()
    @Published var pairingError: String?
    var machines: [SavedMachine] { registry.machines }
    var selectedMachine: SavedMachine? { registry.selected }
    var machineKey: String { selectedMachine?.stateKey ?? endpoint }
    private var selectionGeneration = 0
    var selectionToken: Int { selectionGeneration }
    func isSelected(_ token: Int) -> Bool { token == selectionGeneration }
    private var switchingMachine = false
    private var registryUnavailable = false
    private var machineSubmissions: [String: [Submission]] = [:]
    private var machineCanceledIds: [String: [String]] = [:]
    private var cachedSnapshots: [String: Snapshot] = [:]
    private var machineEditorOffers: [String: String] = [:]
    private var machinePrincipals: [String: String] = [:]
    private var repairedMachines: Set<String> = []
    private var machineEditorIds: [String: Set<String>] = [:]
    private var machineSessions: [String: String] = [:]

    private var socket: URLSessionWebSocketTask?
    private var receiveTask: Task<Void, Never>?
    private var retryTask: Task<Void, Never>?
    private var pending: [String: CheckedContinuation<JSONValue, Error>] = [:]
    private var agentRequests: [String: Int] = [:]
    private var timeouts: [String: Task<Void, Never>] = [:]
    private var drafts: [String: String] = [:]
    private var attachmentDrafts: [String: [ComposerAttachment]] = [:]
    private let writer = CoalescedWriter()
    private var recoveryTask: Task<Void, Never>?
    private var connectionTask: Task<Void, Never>?
    private var recoveryLoaded = false
    private var recoveryFailed = false
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
    var canSend: Bool { connected && chatGrant.canSend && !recoveryFailed && snapshot?.error == nil && (!demo || !busy) && !changingSession && (!busy || attachments.isEmpty) && (!draft.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || !attachments.isEmpty) }
    var draftKey: String { machineKey + ":" + (snapshot?.sessionId ?? machineSessions[machineKey] ?? "local") }
    var recoverable: [Submission] { submissions.filter { $0.recoverable } }

    private(set) var clientId: String

    private let trustWriter: PairingTrustWriter
    private let trustLoader: @Sendable (String) throws -> TrustedHost?
    private let defaults: RecoveryPreferences
    private let recoverySaver: @Sendable ([String: [Submission]]) throws -> Void
    private let enrollment: (@Sendable (PairingCode) async throws -> TrustedHost)?

    init(defaults: UserDefaults = .standard, trustLoader: @escaping @Sendable (String) throws -> TrustedHost? = { try PairingSecret.readTrust(endpoint: $0) }, trustSaver: (@Sendable (TrustedHost) throws -> Void)? = nil,
         trustRemover: @escaping @Sendable (String) throws -> Void = { try PairingSecret.removeTrust(endpoint: $0) },
         enrollment: (@Sendable (PairingCode) async throws -> TrustedHost)? = nil,
         recoveryLoader: (@Sendable () async throws -> ([String: [ComposerAttachment]], [String: [Submission]]))? = nil,
         recoverySaver: @escaping @Sendable ([String: [Submission]]) throws -> Void = { try LocalRecoveryStore.save($0, name: "machine-submissions-v1") }) {
        if let trustSaver {
            self.trustWriter = PairingTrustWriter(save: trustSaver, load: trustLoader, remove: trustRemover)
        } else {
            self.trustWriter = PairingTrustWriter(save: { try PairingSecret.saveTrust($0) }, load: { try PairingSecret.readTrust(endpoint: $0) }, remove: trustRemover, begin: { try PairingSecret.beginTrustChange(endpoint: $0) }, finish: { try PairingSecret.finishTrustChange(endpoint: $0) }, backup: { try PairingSecret.backupTrust(endpoint: $0) })
        }
        self.enrollment = enrollment
        self.recoverySaver = recoverySaver
        self.trustLoader = trustLoader
        let defaults = RecoveryPreferences(value: defaults)
        self.defaults = defaults
        clientId = defaults.value.string(forKey: "pi.clientId") ?? UUID().uuidString
        defaults.value.set(clientId, forKey: "pi.clientId")
        let legacyEndpoint = defaults.value.string(forKey: "pi.endpoint") ?? ""
        if let stored = defaults.value.data(forKey: "pi.machines.v1") {
            if stored.count <= 1024 * 1024, let decoded = try? JSONDecoder().decode(MachineRegistry.self, from: stored), decoded.valid { registry = decoded }
            else { registryUnavailable = true; error = "Saved computer list could not be read. Local data has been kept. Re-pair deliberately to reconnect." }
        } else { registry = MachineRegistry.migrate(endpoint: legacyEndpoint) }
        endpoint = registry.selected?.endpoint ?? (registryUnavailable ? legacyEndpoint : "")
        machineSessions = defaults.value.dictionary(forKey: "pi.machineSessions") as? [String: String] ?? [:]
        machineCanceledIds = defaults.value.dictionary(forKey: "pi.machineCanceledIds") as? [String: [String]] ?? [:]
        if machineCanceledIds.isEmpty && registry.selected?.id.hasPrefix("legacy:") == true { machineCanceledIds[machineKey] = defaults.value.stringArray(forKey: "pi.canceledIds") ?? [] }
        drafts = defaults.value.dictionary(forKey: "pi.drafts") as? [String: String] ?? [:]
        let savedCanceledIds = machineCanceledIds[machineKey] ?? []
        canceledIdHistory = Self.recentCanceledIds(savedCanceledIds)
        seenCanceledIds = Set(canceledIdHistory)
        draft = drafts[draftKey] ?? ""
        if canceledIdHistory != savedCanceledIds || defaults.value.dictionary(forKey: "pi.machineCanceledIds") == nil { saveCanceledIds() }
        persistRegistry()
        let legacyRecoveryKey = machineKey
        networkMonitor.pathUpdateHandler = { [weak self] path in
            let reachable = path.status == .satisfied
            Task { @MainActor [weak self] in self?.networkChanged(reachable: reachable) }
        }
        networkMonitor.start(queue: DispatchQueue(label: "pointguard.reachability"))
        recoveryTask = Task { [weak self] in
            do {
                let saved: ([String: [ComposerAttachment]], [String: [Submission]])
                if let recoveryLoader { saved = try await recoveryLoader() }
                else { saved = try await Task.detached(priority: .utility) {
                    let receipts = try LocalRecoveryStore.read("machine-submissions-v1", as: [String: [Submission]].self) ?? [legacyRecoveryKey: LocalRecoveryStore.read("submissions", as: [Submission].self) ?? []]
                    return (try LocalRecoveryStore.read("attachment-drafts", as: [String: [ComposerAttachment]].self) ?? [:], receipts)
                }.value }
                guard let self else { return }
                // Edits made while loading always win over saved recovery.
                self.attachmentDrafts = saved.0.merging(self.attachmentDrafts) { _, current in current }
                for (key, items) in saved.1 {
                    let current = self.machineSubmissions[key] ?? (key == self.machineKey ? self.submissions : [])
                    let currentIds = Set(current.map(\.id))
                    self.machineSubmissions[key] = Self.retainedSubmissions(items.filter { !currentIds.contains($0.id) }.map { item in
                        var item = item
                        if item.status == "Sending" || (item.status == "Queued" && (self.repairedMachines.contains(key) || (item.clientId != nil && self.machinePrincipals[key] != nil && item.clientId != self.machinePrincipals[key]))) { item.status = "Delivery uncertain · Inspect history on the computer"; item.recoverable = true }
                        return item
                    } + current)
                }
                self.submissions = self.machineSubmissions[self.machineKey] ?? self.submissions
                self.attachments = self.attachmentDrafts[self.draftKey] ?? self.attachments
                self.recoveryLoaded = true
                self.saveSubmissions(); self.saveAttachmentDraft()
            } catch {
                self?.recoveryFailed = true
                self?.error = "Could not open saved message recovery. Keep this phone’s local data and resolve its storage issue before sending."
            }
        }
    }
    deinit { networkMonitor.cancel() }
    private static func recentCanceledIds(_ ids: [String]) -> [String] {
        var seen = Set<String>()
        return Array(ids.reversed().filter { seen.insert($0).inserted }.prefix(1000).reversed())
    }
    private func saveCanceledIds() {
        machineCanceledIds[machineKey] = canceledIdHistory
        let ids = machineCanceledIds
        let defaults = defaults
        writer.schedule(key: "canceled") { defaults.value.set(ids, forKey: "pi.machineCanceledIds") }
    }
    private func saveDraft() {
        guard !switchingMachine else { return }
        drafts[draftKey] = String(draft.prefix(65536))
        if drafts.keys.filter({ $0.hasPrefix(machineKey + ":") }).count > 100 { drafts.removeValue(forKey: drafts.keys.first(where: { $0.hasPrefix(machineKey + ":") && $0 != draftKey }) ?? "") }
        let value = drafts
        let defaults = defaults
        writer.schedule(key: "drafts") { defaults.value.set(value, forKey: "pi.drafts") }
    }
    private static func retainedSubmissions(_ items: [Submission]) -> [Submission] {
        // In-flight sends outrank recovery, which outranks settled receipts. Within
        // each priority retain the newest, then preserve chronological display order.
        func priority(_ item: Submission) -> Int { item.status == "Sending" ? 2 : (item.recoverable || item.status == "Queued") ? 1 : 0 }
        let kept = Set(items.indices.sorted {
            let left = priority(items[$0]), right = priority(items[$1])
            return left == right ? $0 > $1 : left > right
        }.prefix(50))
        return items.enumerated().compactMap { index, item in
            guard kept.contains(index) else { return nil }
            var item = item
            if !item.recoverable && item.status != "Sending" && item.status != "Queued" { item.attachments = nil }
            return item
        }
    }
    private func saveSubmissions() {
        guard !switchingMachine else { return }
        machineSubmissions[machineKey] = submissions
        guard recoveryLoaded else { return }
        let value = machineSubmissions
        let token = selectionGeneration
        let save = recoverySaver
        writer.schedule(key: "submissions") { [weak self] in
            do { try save(value) }
            catch { Task { @MainActor [weak self] in
                if self?.isSelected(token) == true && self?.error?.hasPrefix("Message not sent:") != true { self?.error = "Could not save message recovery on this phone." }
            } }
        }
    }
    private func saveAttachmentDraft() {
        guard !switchingMachine else { return }
        attachmentDrafts[draftKey] = attachments
        while attachmentDrafts.keys.filter({ $0.hasPrefix(machineKey + ":") }).count > 10, let key = attachmentDrafts.keys.first(where: { $0.hasPrefix(machineKey + ":") && $0 != draftKey }) { attachmentDrafts.removeValue(forKey: key) }
        guard recoveryLoaded else { return }
        let value = attachmentDrafts
        let token = selectionGeneration
        writer.schedule(key: "attachments") { [weak self] in
            do { try LocalRecoveryStore.save(value, name: "attachment-drafts") }
            catch { Task { @MainActor [weak self] in if self?.isSelected(token) == true { self?.error = "Could not save attachments on this phone. Keep the app open until sending." } } }
        }
    }
    func addAttachment(_ file: ComposerAttachment) {
        guard attachments.count < 4, attachments.reduce(file.data.count, { $0 + $1.data.count }) <= 512 * 1024 else { error = "Attach up to four files, totaling 512 KiB after preparation."; return }
        attachments.append(file)
    }
    private func persistRegistry() {
        guard !registryUnavailable else { return }
        defaults.value.set(try? JSONEncoder().encode(registry), forKey: "pi.machines.v1")
        defaults.value.set(endpoint, forKey: "pi.endpoint")
    }
    func renameMachine(_ id: String, name: String) {
        guard let index = registry.machines.firstIndex(where: { $0.id == id }) else { return }
        let name = SavedMachine.normalizedName(name)
        guard !name.isEmpty else { return }
        registry.machines[index].name = name; persistRegistry()
    }
    func cancelPairing() {
        pairingTask?.cancel(); pairingGeneration += 1; trustWriter.advance(to: pairingGeneration)
        pairingTask = nil; pairing = false; pairingError = nil
    }
    func selectMachine(_ id: String) {
        guard let machine = machines.first(where: { $0.id == id }) else { return }
        cancelPairing()
        if registry.selectedId == id { connect(); return }
        saveActiveMachine()
        registry.selectedId = id
        activateMachine(machine)
    }
    private func saveActiveMachine() {
        saveDraft(); saveAttachmentDraft(); saveSubmissions(); saveCanceledIds()
        machineEditorIds[machineKey] = seenEditorIds
        machineEditorOffers[machineKey] = editorOffer
        if let state = snapshot {
            cachedSnapshots[machineKey] = Snapshot(version: state.version, epoch: state.epoch, revision: state.revision, busy: false, sessionId: state.sessionId, title: state.title, messages: state.messages, tools: state.tools, queue: [], dialogs: [], notices: [], capabilities: state.capabilities)
        }
    }
    private func activatePairedHost(_ trust: TrustedHost) {
        saveActiveMachine()
        if registryUnavailable {
            if defaults.value.data(forKey: "pi.machines.recovery.v1") == nil { defaults.value.set(defaults.value.data(forKey: "pi.machines.v1"), forKey: "pi.machines.recovery.v1") }
            registry = MachineRegistry.migrate(endpoint: endpoint); registryUnavailable = false
        }
        let machine = registry.remember(hostId: trust.hostId, endpoint: trust.endpoint)
        adoptPrincipal(trust.clientId, machine: machine.stateKey, pairedAgain: true)
        activateMachine(machine)
    }
    private func activateMachine(_ machine: SavedMachine) {
        retryTask?.cancel(); detach(); selectionGeneration += 1; switchingMachine = true
        endpoint = machine.endpoint; demo = false; clientId = ""; wantsConnection = false
        snapshot = cachedSnapshots[machineKey]; cursor.reset(); conversations = []; browsing = nil; editorOffer = machineEditorOffers[machineKey]
        seenEditorIds = machineEditorIds[machineKey] ?? []; changingSession = false; error = nil; pairingError = nil
        canceledIdHistory = Self.recentCanceledIds(machineCanceledIds[machineKey] ?? [])
        seenCanceledIds = Set(canceledIdHistory)
        submissions = machineSubmissions[machineKey] ?? []
        draft = drafts[draftKey] ?? ""; attachments = attachmentDrafts[draftKey] ?? []
        switchingMachine = false; saveSubmissions(); persistRegistry(); connect()
    }
    private func adoptPrincipal(_ principal: String, machine: String, pairedAgain: Bool = false) {
        machinePrincipals[machine] = principal
        if pairedAgain { repairedMachines.insert(machine) }
        machineSubmissions[machine] = (machineSubmissions[machine] ?? []).map { item in
            var item = item
            if (item.status == "Queued" || item.status == "Sending") && (pairedAgain || (item.clientId != nil && item.clientId != principal)) {
                item.status = "Delivery uncertain after re-pair · Inspect history on the computer"; item.recoverable = true
            }
            return item
        }
    }
    // Bind migrated metadata only after the saved credential and pin verify this identity.
    private func bindVerifiedHost(_ trust: TrustedHost) throws {
        if let selected = selectedMachine, !selected.id.hasPrefix("legacy:"), selected.id != trust.hostId {
            throw TrustFailure("Selected computer identity changed.")
        }
        if selectedMachine?.id.hasPrefix("legacy:") == true {
            _ = registry.remember(hostId: trust.hostId, endpoint: trust.endpoint); persistRegistry()
        }
    }

    func pair(_ code: PairingCode) {
        guard PairingCode.parse((try? String(data: JSONEncoder().encode(PairingWire(code)), encoding: .utf8)) ?? "") != nil else { pairingError = "This pairing code expired. Create a new one and try again."; return }
        pairingTask?.cancel(); pairingGeneration += 1; trustWriter.advance(to: pairingGeneration); let attempt = pairingGeneration
        pairing = true; pairingError = nil
        pairingTask = Task { [weak self] in
            let pin = PinnedHostSession(endpoint: code.endpoint, certificateSHA256: code.certificateSHA256)
            let session = pin.makeSession(); defer { session.invalidateAndCancel() }
            do {
                let saved: TrustedHost
                if let enrollment = self?.enrollment { saved = try await enrollment(code) }
                else {
                var request = URLRequest(url: Self.httpURL(code.endpoint, path: "/pair/enroll"))
                request.httpMethod = "POST"; request.setValue("application/json", forHTTPHeaderField: "Content-Type")
                request.httpBody = try JSONSerialization.data(withJSONObject: ["version": 1, "enrollmentToken": code.enrollmentToken, "name": "iPhone"])
                let (data, response) = try await session.data(for: request)
                guard (response as? HTTPURLResponse)?.statusCode == 200 else { throw TrustFailure("Enrollment expired, used or revoked.") }
                let device = try JSONDecoder().decode(EnrolledDevice.self, from: data)
                guard device.version == 1, device.hostId == code.hostId else { throw TrustFailure("Computer identity changed.") }
                saved = TrustedHost(version: 1, endpoint: code.endpoint, hostId: code.hostId, certificateSHA256: code.certificateSHA256, deviceId: device.deviceId, clientId: device.clientId, credential: device.credential)
                }
                guard saved.valid, saved.endpoint == code.endpoint, saved.hostId == code.hostId, saved.certificateSHA256 == code.certificateSHA256 else { throw TrustFailure("The computer returned mismatched device trust.") }
                guard let self, !Task.isCancelled, attempt == self.pairingGeneration else { return }
                try await self.trustWriter.commit(saved, generation: attempt)
                guard !Task.isCancelled, attempt == self.pairingGeneration, self.trustWriter.accept(generation: attempt) else { return }
                self.activatePairedHost(saved)
                self.pairing = false; self.pairingTask = nil
            } catch {
                guard let self, attempt == self.pairingGeneration else { return }
                self.pairing = false; self.pairingTask = nil
                self.pairingError = pin.identityRejected ? "The computer's certificate does not match this pairing code. Create a new code deliberately." : "Pairing failed. " + error.localizedDescription + " Create a new pairing code; enrollment is never replayed."
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
        guard !registryUnavailable else { return }
        guard !demo, foreground, socket == nil, connectionTask == nil else { return }
        guard let url = ConnectionAddress.url(endpoint), url.scheme == "wss" else {
            if !endpoint.isEmpty { requirePairing("Legacy or invalid saved trust.") }
            return
        }
        if resetRetryBudget { retryBudget.reset() }
        wantsConnection = true; retryTask?.cancel()
        let address = endpoint; let loadTrust = trustLoader; generation += 1; let current = generation
        connectionStatus = "Connecting to your computer…"
        connectionTask = Task { [weak self] in
            await self?.recoveryTask?.value
            await self?.trustWriter.drain()
            var session: URLSession?; var pin: PinnedHostSession?
            do {
                guard let saved = try await Task.detached(priority: .userInitiated, operation: { try loadTrust(address) }).value else { throw TrustFailure("No saved device trust.") }
                guard let self, !Task.isCancelled, current == self.generation else { return }
                guard saved.valid, saved.endpoint == address else { throw TrustFailure("Saved trust for this computer is invalid.") }
                if let selected = self.selectedMachine, !selected.id.hasPrefix("legacy:"), selected.id != saved.hostId { throw TrustFailure("Selected computer identity changed.") }
                let delegate = PinnedHostSession(endpoint: address, certificateSHA256: saved.certificateSHA256)
                let connection = delegate.makeSession(); session = connection; pin = delegate
                var verify = URLRequest(url: Self.httpURL(address, path: "/pair/verify"))
                verify.setValue("Bearer " + saved.credential, forHTTPHeaderField: "Authorization")
                let (data, response) = try await connection.data(for: verify)
                let status = (response as? HTTPURLResponse)?.statusCode
                if status == 401 || status == 403 { throw TrustFailure("This phone’s trust was revoked or lost.") }
                guard status == 200 else { throw MobileError("Trust verification is unavailable on this computer.") }
                let verified = try JSONDecoder().decode(VerifiedDevice.self, from: data)
                guard verified.version == 1, verified.hostId == saved.hostId, verified.deviceId == saved.deviceId, verified.clientId == saved.clientId else { throw TrustFailure("Saved computer identity changed.") }
                let host = HostIdentity.fromVerify(data)
                guard !Task.isCancelled, current == self.generation, self.foreground, self.wantsConnection, self.endpoint == address else { connection.invalidateAndCancel(); return }
                try self.bindVerifiedHost(saved)
                if self.registry.describe(hostId: saved.hostId, as: host) { self.persistRegistry() }
                self.adoptPrincipal(saved.clientId, machine: self.machineKey)
                self.submissions = self.machineSubmissions[self.machineKey] ?? self.submissions
                self.connectionTask = nil; self.clientId = saved.clientId; self.hostSession = connection; self.hostPin = delegate
                self.startConnection(url: url, credential: saved.credential, session: connection)
            } catch {
                session?.invalidateAndCancel()
                guard let self, !Task.isCancelled, current == self.generation else { return }
                self.connectionTask = nil
                if pin?.identityRejected == true || error is TrustFailure { self.requirePairing(pin?.identityRejected == true ? "The computer's certificate changed." : error.localizedDescription) }
                else { self.connectionLost(error.localizedDescription) }
            }
        }
    }
    private func startConnection(url: URL, credential: String, session: URLSession) {
        generation += 1; let current = generation
        cursor.reset(); connectionStatus = "Connecting to your computer…"; error = nil
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
                if task.closeCode.rawValue == 4001 || self.hostPin?.identityRejected == true { self.requirePairing("Trust changed or this phone was revoked.") }
                else { self.connectionLost(error.localizedDescription) }
            }
        }
        Task { [weak self] in
            guard let self else { return }
            do {
                // The bridge pushes this phone's grants before any receipt, so a sync the
                // host refused for chat Off is settled here and agents still connect.
                do { try await self.syncState() } catch where current == self.generation && !self.chatGrant.canRead {}
                guard current == self.generation else { return }
                let transport = self.generation
                self.agentControl.transport(host: self.machineKey, connected: true) { [weak self] op, fields in
                    guard let self, self.generation == transport, self.socket != nil else { throw MobileError("Agent connection changed. Reopen the agent before continuing.") }
                    let id = UUID().uuidString
                    self.agentRequests[id] = transport
                    defer { self.agentRequests.removeValue(forKey: id) }
                    return try await self.call(op, fields: fields, id: id)
                }
                // Terminal discovery and remote fleet health never gate Pi chat.
                self.connected = true; self.connectionStatus = "Connected to your computer"; self.retryBudget.reset()
            } catch { if current == self.generation { self.connectionLost(error.localizedDescription) } }
        }
    }
    func setForeground(_ active: Bool) {
        foreground = active
        if active { retryBudget.reset(); if wantsConnection { connect(resetRetryBudget: false) } }
        else {
            writer.flush(); retryTask?.cancel(); cancelPairing()
            detach(); if wantsConnection { connectionStatus = "Paused on this phone · Point Guard continues on your computer" }
        }
    }
    private func networkChanged(reachable: Bool) {
        guard foreground, wantsConnection else { return }
        if reachable { retryBudget.reset(); if socket == nil { connect(resetRetryBudget: false) } }
        else { retryTask?.cancel(); detach(); connectionStatus = "Waiting for network · Point Guard continues on your computer" }
    }
    func disconnect(cancelPairing: Bool = true) {
        if cancelPairing {
            self.cancelPairing()
        }
        wantsConnection = false; retryTask?.cancel(); detach(); connectionStatus = "Not connected"
    }
    private func detach() {
        agentControl.transport(host: machineKey, connected: false)
        generation += 1; connectionTask?.cancel(); connectionTask = nil; connected = false; receiveTask?.cancel(); receiveTask = nil; socket?.cancel(with: .goingAway, reason: nil); socket = nil
        hostSession?.invalidateAndCancel(); hostSession = nil; hostPin = nil
        let outstanding = pending; pending = [:]; for timeout in timeouts.values { timeout.cancel() }; timeouts = [:]
        for continuation in outstanding.values { continuation.resume(throwing: MobileError("Connection interrupted. Delivery may be uncertain. Inspect the conversation before sending again.")) }
    }
    /// Re-pair guidance worded for the selected computer's verified, display-only kind.
    var repairHint: String { HostCopy.repairHint(kind: selectedMachine?.host.kind, label: selectedMachine?.host.label) }
    private func requirePairing(_ reason: String) {
        agentControl.revokeAccess()
        wantsConnection = false; retryTask?.cancel(); detach(); connectionStatus = "Re-pair needed"; error = reason + " " + repairHint
    }
    private func connectionLost(_ reason: String) {
        retryTask?.cancel(); detach(); connectionStatus = "Connection lost · Point Guard continues on your computer"; error = reason + " Check PurePoint on the computer and Tailscale."
        guard foreground, wantsConnection else { return }
        guard let delay = retryBudget.nextDelay() else { connectionStatus = "Reconnect paused · Check the computer and network"; return }
        let token = selectionGeneration
        retryTask = Task { [weak self] in try? await Task.sleep(nanoseconds: delay * 1_000_000_000); guard !Task.isCancelled, self?.isSelected(token) == true else { return }; self?.connect(resetRetryBudget: false) }
    }
    private func receive(_ incoming: IncomingRecord) throws {
        let record: WireEnvelope
        switch incoming {
        case .snapshot(let state, let rows): try apply(state, rows: rows); return
        case .envelope(let envelope): record = envelope
        }
        if record.type == "permissions" { applyChatGrant(ChatGrant.from(record.chat), resync: true) }
        if record.type == "permissions" || record.type.hasPrefix("terminal.") { agentControl.receive(record); return }
        switch record.type {
        case "receipt":
            guard let id = record.id, let continuation = pending.removeValue(forKey: id) else { return }
            timeouts.removeValue(forKey: id)?.cancel()
            if record.ok == true { continuation.resume(returning: record.data ?? .null) } else { continuation.resume(throwing: MobileError(record.error ?? "The computer rejected this action.")) }
        case "editor": if let id = record.id, !seenEditorIds.contains(id) { seenEditorIds.insert(id); editorOffer = record.text }
        default: throw MobileError("Unsupported bridge response. Update the app and bridge together.")
        }
    }
    /// A downgrade hides chat content at once; regaining read access fetches the current conversation.
    private func applyChatGrant(_ grant: ChatGrant, resync: Bool) {
        let previous = chatGrant
        chatGrant = grant
        if !grant.canRead {
            saveDraft(); snapshot = nil; cursor.reset(); editorOffer = nil; browsing = nil; conversations = []
        } else if resync && !previous.canRead && connected {
            let current = generation
            Task { [weak self] in
                do { try await self?.syncState() } catch { if let self, current == self.generation { self.error = error.localizedDescription } }
            }
        }
    }
    private func apply(_ state: Snapshot, rows: [TranscriptRow]? = nil) throws {
        guard chatGrant.canRead else { return }
        guard state.version == 1 else { throw MobileError("Unsupported bridge version. Update the app and bridge together.") }
        guard cursor.accept(epoch: state.epoch, revision: state.revision) else { return }
        let previous = snapshot?.sessionId
        let localDraft = draft
        let localAttachments = attachments
        let oldKey = draftKey
        if previous != state.sessionId { saveDraft() }
        if let rows { projectingSnapshot = true; transcriptRows = rows }
        snapshot = state
        machineSessions[machineKey] = state.sessionId
        defaults.value.set(machineSessions, forKey: "pi.machineSessions")
        projectingSnapshot = false
        let previousCanceled = canceledIdHistory
        for canceled in state.canceled ?? [] where canceled.clientId == clientId && !seenCanceledIds.contains(canceled.id) {
            submissions.append(Submission(id: canceled.id, text: canceled.text, sessionId: canceled.sessionId, status: "Canceled by Stop", recoverable: true, clientId: canceled.clientId))
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
        guard let socket else { throw MobileError("Connect to your computer before sending.") }
        var record = fields; record["version"] = 1
        record["clientId"] = clientId; record["id"] = id; record["op"] = op
        let data = try JSONSerialization.data(withJSONObject: record)
        guard let text = String(data: data, encoding: .utf8) else { throw MobileError("Could not encode the message.") }
        return try await withCheckedThrowingContinuation { continuation in
            pending[id] = continuation
            timeouts[id] = Task { [weak self] in
                try? await Task.sleep(nanoseconds: MobileRPCTiming.timeoutNanoseconds(for: op))
                guard !Task.isCancelled, let self, let continuation = self.pending.removeValue(forKey: id) else { return }
                self.timeouts.removeValue(forKey: id); continuation.resume(throwing: MobileError(op == "agent.prompt" ? "Delivery uncertain: no prompt acknowledgement. Inspect the agent terminal before sending again. Your submitted text remains saved." : "No acknowledgement from your computer. Delivery is uncertain; inspect history before sending again."))
            }
            Task { [weak self] in
                do { try await socket.send(.string(text)) }
                catch { guard let self, let continuation = self.pending.removeValue(forKey: id) else { return }; self.timeouts.removeValue(forKey: id)?.cancel(); continuation.resume(throwing: error) }
            }
        }
    }
    func submit(mode: String) {
        guard canSend, let state = snapshot else { return }
        guard submissions.filter({ $0.recoverable || $0.status == "Sending" || $0.status == "Queued" }).count < 50 else {
            error = "Message recovery is full. Restore or dismiss a saved recovery before sending another message."
            return
        }
        let text = draft
        let files = attachments
        let content: ComposerPayload
        do { content = try ComposerPayload.make(text: text, attachments: files) } catch { self.error = error.localizedDescription; return }
        guard content.images.isEmpty || demo || state.capabilities?.contains("images") == true else { error = "Update and restart Point Guard on the computer before sending images."; return }
        let id = UUID().uuidString
        let origin = machineKey; let token = selectionGeneration; let transport = generation
        submissions.append(Submission(id: id, text: text, sessionId: state.sessionId, status: "Sending", recoverable: false, attachments: files, clientId: clientId)); draft = ""; attachments = []
        Task {
            // Hydration and confirmed persistence precede any transmission, while
            // the composer remains editable. Sending receipts cannot be dismissed.
            await recoveryTask?.value
            let receipts = machineSubmissions
            let save = recoverySaver
            do {
                guard recoveryLoaded else { throw MobileError("Saved recovery could not be opened.") }
                try await writer.writeAndConfirm { try save(receipts) }
            } catch {
                let guidance = "Message not sent: could not save recovery on this phone. Free storage, then restore the saved message and try again. Keep the app open to retain its text and files."
                updateSubmission(id, status: guidance, recoverable: true, machine: origin); if isSelected(token) { self.error = guidance }
                return
            }
            do {
                guard isSelected(token), transport == generation else { throw MobileError("Connection changed before sending. Restore this message on its original computer.") }
                if demo { await demoReply(content.message); updateSubmission(id, status: "Accepted", recoverable: false, machine: origin); return }
                let data = try await call("send", fields: ["epoch": state.epoch, "text": content.message, "mode": mode, "images": content.images], id: id)
                let disposition = data["disposition"].text ?? "accepted"
                updateSubmission(id, status: disposition == "queued" ? "Queued" : disposition == "handled" ? "Handled by extension" : "Accepted", recoverable: false, machine: origin)
            } catch { updateSubmission(id, status: error.localizedDescription, recoverable: true, machine: origin); if isSelected(token), transport == generation { self.error = error.localizedDescription } }
        }
    }
    private func updateSubmission(_ id: String, status: String, recoverable: Bool, machine: String? = nil) {
        let key = machine ?? machineKey
        func updated(_ item: Submission) -> Submission {
            var item = item; item.status = status; item.recoverable = recoverable
            if status == "Queued", let principal = machinePrincipals[key], item.clientId != principal {
                item.status = "Delivery uncertain after re-pair · Inspect history on the computer"; item.recoverable = true
            }
            return item
        }
        if key == machineKey {
            if let i = submissions.firstIndex(where: { $0.id == id }) { submissions[i] = updated(submissions[i]) }
        } else if let i = machineSubmissions[key]?.firstIndex(where: { $0.id == id }), let item = machineSubmissions[key]?[i] {
            machineSubmissions[key]?[i] = updated(item)
            machineSubmissions[key] = Self.retainedSubmissions(machineSubmissions[key] ?? [])
            saveSubmissions()
        }
    }
    func restore(_ submission: Submission) {
        guard let saved = submissions.first(where: { $0.id == submission.id }), saved.status != "Sending" else { error = "Wait for this message's delivery result before restoring it."; return }
        let files = (saved.attachments ?? []).filter { file in !attachments.contains(where: { $0.id == file.id }) }
        guard attachments.count + files.count <= 4, (attachments + files).reduce(0, { $0 + $1.data.count }) <= 512 * 1024 else { error = "Remove a draft attachment before restoring these files."; return }
        var state = DraftState(text: draft); state.restore(saved.text); draft = state.text; attachments += files; submissions.removeAll { $0.id == submission.id }
    }
    func dismissSubmission(_ id: String) {
        guard submissions.first(where: { $0.id == id })?.status != "Sending" else { error = "Wait for this message's delivery result before dismissing it."; return }
        submissions.removeAll { $0.id == id }
    }
    func useEditorOffer() { guard let offer = editorOffer else { return }; var state = DraftState(text: draft); state.restore(offer); draft = state.text; editorOffer = nil }
    func stop(selection: Int? = nil) async -> Bool {
        if let selection, !isSelected(selection) { return false }
        let current = generation
        guard chatGrant.canSend else { error = chatGrant.explanation; return false }
        guard let state = snapshot, let run = state.runId else { return !busy }
        do {
            _ = try await call("stop", fields: ["epoch": state.epoch, "runId": run])
            guard current == generation else { return false }; try await syncState(); return true
        } catch { if current == generation { self.error = error.localizedDescription }; return false }
    }
    func loadConversations(selection: Int? = nil) async {
        if let selection, !isSelected(selection) { return }
        let current = generation
        if demo { conversations = [Conversation(id: "previous", title: "A previous thought", date: nil)]; return }
        guard chatGrant.canRead else { error = chatGrant.explanation; return }
        do { let value = try await call("sessions")["sessions"]; let result = try await Task.detached(priority: .userInitiated) { try value.decode([Conversation].self) }.value; guard current == generation else { return }; conversations = result } catch { if current == generation { self.error = error.localizedDescription } }
    }
    func browse(_ conversation: Conversation, selection: Int? = nil) async {
        if let selection, !isSelected(selection) { return }
        let current = generation
        guard chatGrant.canRead else { error = chatGrant.explanation; return }
        if demo { browsing = History(sessionId: conversation.id, title: conversation.title, messages: [ChatMessage(id: "old", role: "assistant", text: "There is room here for a fresh idea.")]); return }
        do { let value = try await call("history", fields: ["sessionId": conversation.id]); let result = try await Task.detached(priority: .userInitiated) { try value.decode(History.self) }.value; guard current == generation else { return }; browsing = result } catch { if current == generation { self.error = error.localizedDescription } }
    }
    func changeSession(to id: String? = nil, stopFirst: Bool = false, selection: Int? = nil) async {
        if let selection, !isSelected(selection) { return }
        let current = generation
        guard chatGrant.canSend else { error = chatGrant.explanation; return }
        guard !changingSession, let state = snapshot else { return }; changingSession = true; defer { if current == generation { changingSession = false } }
        if busy { guard stopFirst, await stop() else { return } }
        guard current == generation else { return }
        do {
            var fields: [String: Any] = ["epoch": state.epoch]; if let id { fields["sessionId"] = id }
            _ = try await call(id == nil ? "new" : "resume", fields: fields)
            guard current == generation else { return }; try await syncState(); browsing = nil
        } catch { if current == generation { self.error = error.localizedDescription } }
    }
    func answer(_ dialog: ExtensionDialog, value: String? = nil, optionId: String? = nil, confirmed: Bool? = nil, cancelled: Bool = false, selection: Int? = nil) async {
        if let selection, !isSelected(selection) { return }
        let current = generation
        guard chatGrant.canSend else { error = chatGrant.explanation; return }
        guard let state = snapshot, state.dialogs.contains(where: { $0.id == dialog.id }) else { return }
        var fields: [String: Any] = ["epoch": state.epoch, "dialogId": dialog.id, "cancelled": cancelled]; if let optionId { fields["optionId"] = optionId }; if let value { fields["value"] = value }; if let confirmed { fields["confirmed"] = confirmed }
        do { _ = try await call("answer", fields: fields) } catch { if current == generation { self.error = error.localizedDescription } }
    }
    func exploreDemo() {
        disconnect(); demo = true; chatGrant = .send; connected = true; connectionStatus = "On-device preview"; error = nil; cursor.reset()
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
