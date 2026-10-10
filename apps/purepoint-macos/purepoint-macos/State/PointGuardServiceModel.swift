import Combine
import Foundation

private final class PointGuardHTTPDelegate: NSObject, URLSessionTaskDelegate, @unchecked Sendable {
    func urlSession(_ session: URLSession, task: URLSessionTask, willPerformHTTPRedirection response: HTTPURLResponse,
                    newRequest request: URLRequest, completionHandler: @escaping @Sendable (URLRequest?) -> Void) {
        completionHandler(nil)
    }
}

/// Owns only the bridge this app launched; descriptors never authorize process adoption.
@MainActor final class PointGuardServiceModel: ObservableObject {
    @Published private(set) var phase = "Stopped"
    @Published var error: String?
    @Published private(set) var ready = false
    @Published private(set) var providers: [PiJSONValue] = []
    @Published private(set) var auth: PiJSONValue = .null
    @Published private(set) var enrollment: PiJSONValue = .null
    @Published private(set) var devices: [PiJSONValue] = []
    @Published private(set) var restartRequired = false
    @Published private(set) var cwd = ""
    @Published private(set) var selectedProvider = ""
    @Published private(set) var selectedModel = ""
    @Published private(set) var enrollmentStatus = ""
    @Published private(set) var remoteEndpoint: String?
    private var process: Process?
    private var descriptor: PointGuardDescriptor?
    private var instanceId = ""
    private var adminToken = ""
    private var pollTask: Task<Void, Never>?
    private var authGeneration = 0
    private var enrollmentGeneration = 0
    private var activeAttemptId: String?
    private let requestOverride: (@MainActor (String, [String: PiJSONValue]) async throws -> PiJSONValue)?
    private var startTask: Task<Void, Never>?
    private var expectedStop = false
    private var stopping = false
    private weak var chat: PiChatModel?
    private let stateDirectory: URL
    private let session = URLSession(configuration: .ephemeral, delegate: PointGuardHTTPDelegate(), delegateQueue: nil)

    init(stateDirectory: URL? = nil,
         requestOverride: (@MainActor (String, [String: PiJSONValue]) async throws -> PiJSONValue)? = nil,
         remoteEndpoint: String? = nil) {
        self.requestOverride = requestOverride
        self.remoteEndpoint = remoteEndpoint
        self.stateDirectory = stateDirectory ?? FileManager.default.homeDirectoryForCurrentUser
            .appendingPathComponent("Library/Application Support/PurePoint/PointGuard", isDirectory: true)
    }
    func start(chat: PiChatModel) {
        self.chat = chat
        guard process == nil, startTask == nil, !stopping else { return }
        error = nil; phase = "Starting Pi…"; expectedStop = false
        startTask = Task { [weak self] in
            guard let self else { return }
            defer { self.startTask = nil }
            do {
                guard let resources = Bundle.main.resourceURL else { throw PiChatError("PurePoint bundle resources are unavailable.") }
                let manifestURL = resources.appendingPathComponent("PointGuard/runtime-manifest.json")
                let runtime = try await Task.detached(priority: .userInitiated) { try PointGuardRuntime.load(manifestURL: manifestURL) }.value
                guard !Task.isCancelled else { return }
                let child = Process()
                self.instanceId = UUID().uuidString
                child.executableURL = runtime.node; child.arguments = [runtime.entry.path, "--managed"]
                var environment = ProcessInfo.processInfo.environment
                for name in ["PU_AGENT_ID", "PU_PROJECT_ROOT", "NODE_OPTIONS", "NODE_PATH", "PI_MOBILE_SESSION", "PI_MOBILE_TOKEN_FILE", "PI_MOBILE_PU_SKILL", "PI_MOBILE_CWD"] { environment.removeValue(forKey: name) }
                environment["POINT_GUARD_STATE_DIR"] = self.stateDirectory.path
                environment["POINT_GUARD_PU_PATH"] = runtime.pu.path
                environment["POINT_GUARD_INSTANCE_ID"] = self.instanceId
                environment["PATH"] = runtime.pu.deletingLastPathComponent().path + ":/usr/bin:/bin:/usr/sbin:/sbin"
                // The app discovers the explicit tailnet IP without requiring an external CLI.
                if let address = PointGuardTailnet.address() { environment["PI_MOBILE_HOST"] = address }
                else { environment.removeValue(forKey: "PI_MOBILE_HOST") }
                child.environment = environment
                child.currentDirectoryURL = FileManager.default.homeDirectoryForCurrentUser
                child.standardOutput = FileHandle.nullDevice; child.standardError = FileHandle.nullDevice
                let launchId = self.instanceId
                child.terminationHandler = { [weak self] child in
                    let status = child.terminationStatus
                    Task { @MainActor in
                        guard let self, self.instanceId == launchId else { return }
                        self.ready = false; self.descriptor = nil; self.adminToken = ""; self.process = nil
                        self.chat?.disconnect()
                        self.pollTask?.cancel(); self.pollTask = nil
                        self.phase = "Stopped"
                        if !self.expectedStop {
                            self.phase = "Pi stopped"
                            self.error = "Point Guard exited (\(status)). Retry to restore your saved session. Check private state permissions and available ports if startup fails."
                        }
                    }
                }
                try child.run(); self.process = child
                let descriptor = try await self.awaitReadiness(child, launchId: launchId)
                guard !Task.isCancelled, child.isRunning, self.instanceId == launchId else { return }
                self.descriptor = descriptor
                self.adminToken = try PointGuardPrivateFile.token(self.stateDirectory.appendingPathComponent("admin-token"))
                let chatToken = try PointGuardPrivateFile.token(self.stateDirectory.appendingPathComponent("desktop-chat-token"))
                self.remoteEndpoint = descriptor.chatURL
                chat.bindManagedConnection(endpoint: descriptor.nativeChatURL, secret: chatToken, clientId: descriptor.desktopClientId)
                self.ready = true; self.phase = "Pi ready"
                await self.refresh()
            } catch {
                self.error = error.localizedDescription
                self.phase = "Setup needs attention"
                self.expectedStop = true
                self.process?.terminate() // Only this exact app-owned child; never descriptor PID.
            }
        }
    }
    private func awaitReadiness(_ child: Process, launchId: String) async throws -> PointGuardDescriptor {
        let directory = stateDirectory
        for _ in 0..<300 {
            guard !Task.isCancelled, child.isRunning else { throw PiChatError("Point Guard could not start. Check state permissions, existing runtime ownership and ports, then Retry.") }
            let pid = child.processIdentifier
            let candidate = await Task.detached(priority: .utility) {
                guard let data = try? PointGuardPrivateFile.read(directory.appendingPathComponent("admin.json")),
                    let value = try? JSONDecoder().decode(PointGuardDescriptor.self, from: data), value.belongs(to: pid, instanceId: launchId) else { return Optional<PointGuardDescriptor>.none }
                return value
            }.value
            if let candidate { return candidate }
            try await Task.sleep(for: .milliseconds(100))
        }
        throw PiChatError("Point Guard did not become ready in 30 seconds. Retry after checking its private state and ports.")
    }
    func request(_ operation: String, fields: [String: PiJSONValue] = [:]) async throws -> PiJSONValue {
        if let requestOverride { return try await requestOverride(operation, fields) }
        guard ready, let descriptor, let child = process, child.isRunning,
            descriptor.belongs(to: child.processIdentifier, instanceId: instanceId),
            let url = PointGuardDescriptor.localURL(descriptor.adminURL, scheme: "http", path: "/admin/v1") else { throw PiChatError("Start Point Guard before changing setup.") }
        let launchId = instanceId
        var body = fields; body["operation"] = .string(operation)
        var request = URLRequest(url: url); request.httpMethod = "POST"; request.timeoutInterval = 15
        request.setValue("Bearer " + adminToken, forHTTPHeaderField: "Authorization")
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        request.httpBody = try JSONEncoder().encode(PiJSONValue.object(body))
        let (data, response) = try await session.data(for: request)
        guard launchId == instanceId, data.count <= 1024 * 1024,
            let http = response as? HTTPURLResponse, http.statusCode == 200 else { throw PiChatError("Point Guard setup request failed. Retry after checking the service.") }
        let result = try JSONDecoder().decode(PiJSONValue.self, from: data)
        guard case .bool(true) = result["ok"] else { throw PiChatError(result["error"]["message"].text ?? "Point Guard rejected this setup request.") }
        return result["result"]
    }
    func refresh() async {
        do {
            let status = try await request("status"); cwd = status["cwd"].text ?? ""
            selectedProvider = status["provider"].text ?? ""; selectedModel = status["model"].text ?? ""
            providers = (try await request("providers"))["providers"].values
            devices = (try await request("devices.list"))["devices"].values
        } catch { self.error = error.localizedDescription }
    }
    func login(provider: String, type: String) async {
        authGeneration += 1; let generation = authGeneration
        pollTask?.cancel(); activeAttemptId = nil
        auth = .object(["status": .string("pending")]); error = nil
        do {
            let result = try await request("auth.start", fields: ["provider": .string(provider), "type": .string(type)])
            guard let id = result["attemptId"].text else { throw PiChatError("Provider login did not start.") }
            guard generation == authGeneration else {
                _ = try? await request("auth.cancel", fields: ["attemptId": .string(id)])
                return
            }
            activeAttemptId = id
            pollTask = Task { [weak self] in
                guard let self else { return }
                do {
                    while !Task.isCancelled && generation == self.authGeneration {
                        let status = try await self.request("auth.status", fields: ["attemptId": .string(id)])
                        guard !Task.isCancelled, generation == self.authGeneration else { return }
                        self.auth = status
                        if status["status"].text != "pending" {
                            self.activeAttemptId = nil
                            if status["status"].text == "complete" { self.restartRequired = true; await self.refresh() }
                            return
                        }
                        try await Task.sleep(for: .milliseconds(500))
                    }
                } catch { if !Task.isCancelled && generation == self.authGeneration { self.error = error.localizedDescription } }
            }
        } catch { if generation == authGeneration { self.error = error.localizedDescription; self.auth = .null } }
    }
    func respond(_ value: String) async {
        guard let id = auth["attemptId"].text, let promptId = auth["prompt"]["id"].text else { return }
        do { _ = try await request("auth.respond", fields: ["attemptId": .string(id), "promptId": .string(promptId), "value": .string(value)]) }
        catch { self.error = error.localizedDescription }
    }
    func cancelLogin() async {
        authGeneration += 1; pollTask?.cancel()
        let id = activeAttemptId; activeAttemptId = nil; auth = .null
        guard let id else { return }
        do { _ = try await request("auth.cancel", fields: ["attemptId": .string(id)]) }
        catch { self.error = error.localizedDescription }
    }
    func selectModel(provider: String, model: String) async {
        do { _ = try await request("model.select", fields: ["provider": .string(provider), "model": .string(model)]); await refresh() }
        catch { self.error = error.localizedDescription }
    }
    func configure(cwd: String) async {
        do { _ = try await request("runtime.configure", fields: ["cwd": .string(cwd)]); self.cwd = cwd; restartRequired = true }
        catch { self.error = error.localizedDescription }
    }
    func connectPhone() async {
        enrollmentGeneration += 1; let generation = enrollmentGeneration
        guard let remoteEndpoint else { error = "Connect Tailscale on this Mac, then restart Point Guard to enable phone pairing."; return }
        let oldId = enrollment["enrollmentId"].text
        enrollment = .null; enrollmentStatus = ""
        if let oldId { _ = try? await request("pairing.revoke", fields: ["enrollmentId": .string(oldId)]) }
        guard generation == enrollmentGeneration else { return }
        do {
            let result = try await request("pairing.create", fields: ["endpoint": .string(remoteEndpoint)])
            guard generation == enrollmentGeneration else {
                if let id = result["enrollmentId"].text { _ = try? await request("pairing.revoke", fields: ["enrollmentId": .string(id)]) }
                return
            }
            enrollment = result; enrollmentStatus = "pending"
        } catch { if generation == enrollmentGeneration { self.error = error.localizedDescription } }
    }
    func refreshEnrollment() async {
        guard let id = enrollment["enrollmentId"].text else { return }
        do {
            let status = try await request("pairing.status", fields: ["enrollmentId": .string(id)])
            guard enrollment["enrollmentId"].text == id else { return }
            enrollmentStatus = status["status"].text ?? "unknown"
            if enrollmentStatus == "consumed" { await refresh() }
        } catch { self.error = error.localizedDescription }
    }
    func rotateDevice(_ id: String) async {
        do { _ = try await request("devices.rotate", fields: ["deviceId": .string(id)]); await refresh() }
        catch { self.error = error.localizedDescription }
    }
    func closeEnrollment() async {
        enrollmentGeneration += 1
        let id = enrollment["enrollmentId"].text
        enrollment = .null; enrollmentStatus = ""
        guard let id else { return }
        do { _ = try await request("pairing.revoke", fields: ["enrollmentId": .string(id)]) }
        catch { self.error = error.localizedDescription }
    }
    func revokeDevice(_ id: String) async {
        do { _ = try await request("devices.revoke", fields: ["deviceId": .string(id)]); await refresh() }
        catch { self.error = error.localizedDescription }
    }
    func restart(chat: PiChatModel) async {
        guard !chat.busy else { error = "Wait for Pi to finish or Stop this run before restarting."; return }
        do { try await stop(); restartRequired = false; start(chat: chat) }
        catch { self.error = error.localizedDescription }
    }
    func stop() async throws {
        guard !stopping else { throw PiChatError("Point Guard is already stopping.") }
        stopping = true; defer { stopping = false }
        startTask?.cancel(); pollTask?.cancel(); pollTask = nil
        authGeneration += 1; enrollmentGeneration += 1; activeAttemptId = nil; auth = .null; enrollment = .null
        guard let child = process else { ready = false; phase = "Stopped"; return }
        expectedStop = true
        if ready {
            do {
                // The bridge checks authoritative Pi/queue idleness and freezes new
                // mutations in the same serialized operation before owned shutdown.
                _ = try await request("runtime.stop")
            } catch { expectedStop = false; throw error }
        } else if child.isRunning {
            child.terminate() // Failed startup: still only this exact owned Process.
        }
        ready = false; phase = "Stopping Pi…"; chat?.disconnect()
        for _ in 0..<100 {
            if !child.isRunning { process = nil; descriptor = nil; adminToken = ""; phase = "Stopped"; return }
            try await Task.sleep(for: .milliseconds(100))
        }
        throw PiChatError("Point Guard has not exited. The update/restart is paused until its owned child stops.")
    }
    /// App termination cannot await main-actor tasks; bound the owned child wait.
    func stopForTermination() {
        startTask?.cancel(); pollTask?.cancel(); expectedStop = true
        guard let child = process, child.isRunning else { return }
        child.terminate()
        let deadline = Date().addingTimeInterval(10)
        while child.isRunning && Date() < deadline { Thread.sleep(forTimeInterval: 0.05) }
    }
}
private extension PiJSONValue {
    var values: [PiJSONValue] { if case .array(let values) = self { return values }; return [] }
}
