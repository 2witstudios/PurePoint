import Foundation

nonisolated enum DaemonLifecycle {
    private static let launcher = DaemonLauncher()

    /// Ensure the daemon is running. If not, start it and wait for readiness.
    static func ensureDaemon() async throws {
        try await launcher.ensureDaemon()
    }

    /// Restart only the live child owned by this app, using verified IPC shutdown.
    static func restartDaemon() async throws {
        try await launcher.restartDaemon()
    }

    /// True if this app instance launched the daemon it is talking to.
    /// Used to avoid shutting down a daemon we merely attached to (e.g. a
    /// second app instance quitting must not kill the shared daemon).
    static func didLaunchDaemon() async -> Bool {
        await launcher.ownsRunningDaemon()
    }

    /// Watcher eligibility only, never proof authorizing a lifecycle mutation.
    /// The mutation connection verifies health and the retained child's liveness.
    static func hasLaunchedChild() async -> Bool {
        await launcher.hasChild()
    }

    static func shutdownDaemon(projectRoots: [String]) async throws {
        try await launcher.shutdownDaemon(projectRoots: projectRoots)
    }

    static func findBinary() -> String? {
        // 1. Check app bundle (production path)
        if let bundlePath = Bundle.main.executableURL?
            .deletingLastPathComponent()
            .appendingPathComponent("pu-engine").path,
            FileManager.default.isExecutableFile(atPath: bundlePath)
        {
            return bundlePath
        }

        // 2. Search PATH (standalone/development)
        let pathEnv = ProcessInfo.processInfo.environment["PATH"] ?? "/usr/local/bin:/usr/bin:/bin"
        for dir in pathEnv.split(separator: ":") {
            let candidate = "\(dir)/pu-engine"
            if FileManager.default.isExecutableFile(atPath: candidate) {
                return candidate
            }
        }

        // 3. Cargo bin (development fallback)
        let devPath = "\(FileManager.default.homeDirectoryForCurrentUser.path)/.cargo/bin/pu-engine"
        if FileManager.default.isExecutableFile(atPath: devPath) {
            return devPath
        }

        return nil
    }
}

/// Serializes daemon lifecycle operations so concurrent callers don't race.
///
/// Actor isolation alone is not enough: the actor is reentrant at every `await`,
/// so N projects opening at launch would each see "no daemon yet" while the first
/// launch is still polling health, and each start its own pu-engine. Every
/// operation therefore runs as the single in-flight `Task`, and concurrent callers
/// await that task instead of starting another.
actor DaemonLauncher {
    private let puDir: URL
    private let binaryOverride: String?
    private let probeTimeout: TimeInterval
    private let startupTimeout: TimeInterval
    private var socketPath: String { puDir.appendingPathComponent("daemon.sock").path }
    // Process retains the actual launched child and observes its exit. Advisory
    // PID files, names and timestamps never establish client ownership.
    private var launchedProcess: Process?
    private var inFlight: Task<Void, Error>?

    init(
        stateDirectory: URL = FileManager.default.homeDirectoryForCurrentUser.appendingPathComponent(".pu"),
        binaryPath: String? = nil, probeTimeout: TimeInterval = 1, startupTimeout: TimeInterval = 3
    ) {
        puDir = stateDirectory
        binaryOverride = binaryPath
        self.probeTimeout = probeTimeout
        self.startupTimeout = startupTimeout
    }

    func ensureDaemon() async throws {
        try await serialized(coalesce: true) { try await $0.ensureDaemonNow() }
    }

    func restartDaemon() async throws {
        try await serialized(coalesce: false) { try await $0.restartDaemonNow() }
    }

    func shutdownDaemon(projectRoots: [String]) async throws {
        try await serialized(coalesce: false) { launcher in
            // An attached-only app has nothing to stop on quit.
            guard await launcher.hasChild() else { return }
            try await launcher.stopOwnedDaemon(projectRoots: projectRoots)
        }
    }

    fileprivate func hasChild() -> Bool { launchedProcess != nil }

    func ownsRunningDaemon() async -> Bool {
        guard let process = launchedProcess, process.isRunning else { return false }
        if case .healthy(let pid) = await probe() {
            return process.isRunning && pid == Int(process.processIdentifier)
        }
        return false
    }

    private func serialized(
        coalesce: Bool,
        _ operation: @escaping @Sendable (DaemonLauncher) async throws -> Void
    ) async throws {
        while let running = inFlight {
            let outcome = await running.result
            // The owner clears inFlight only once it is back on the actor; clear a
            // finished task here too so this loop cannot spin on it meanwhile.
            if inFlight == running { inFlight = nil }
            if coalesce { return try outcome.get() }
        }
        let task = Task { try await operation(self) }
        inFlight = task
        defer { if inFlight == task { inFlight = nil } }
        try await task.value
    }

    private func ensureDaemonNow() async throws {
        try DaemonStateDirectory.prepare(puDir)
        switch await probe() {
        case .healthy: return
        case .busy:
            throw DaemonLifecycleError.recoveryRequired(
                "Daemon is busy; compatibility and ownership could not be established.")
        case .incompatible:
            throw DaemonLifecycleError.recoveryRequired("Daemon protocol is incompatible with this app.")
        case .unreachable:
            // A live but unresponsive child must not be replaced. If we merely
            // attached, a fresh candidate may contend under the daemon's flock;
            // the client never removes socket/PID/lock state or signals anyone.
            if launchedProcess?.isRunning == true {
                throw DaemonLifecycleError.recoveryRequired("The app's daemon child is still running but unresponsive.")
            }
            try await launchDaemon()
        }
    }

    private func restartDaemonNow() async throws {
        try await stopOwnedDaemon(projectRoots: [])
        try await launchDaemon()
    }

    private func stopOwnedDaemon(projectRoots: [String]) async throws {
        guard let process = launchedProcess, process.isRunning else {
            throw DaemonLifecycleError.recoveryRequired("This app does not own a live daemon child.")
        }
        try DaemonStateDirectory.prepare(puDir)
        let deadline = DispatchTime.now().uptimeNanoseconds + UInt64(startupTimeout * 1_000_000_000)
        do {
            try await DaemonClient(socketPath: socketPath).withConnection(timeout: startupTimeout) {
                connection, reader in
                try await DaemonClient.write(.health, to: connection)
                guard case .healthReport(let pid, _, let version, _) = DaemonClient.parse(try await reader.readLine()),
                    version == DaemonClient.protocolVersion,
                    process.isRunning, pid == Int(process.processIdentifier)
                else {
                    throw DaemonLifecycleError.recoveryRequired(
                        "Connected daemon is not the app's compatible live child.")
                }
                for root in projectRoots {
                    guard process.isRunning else {
                        throw DaemonLifecycleError.recoveryRequired("Owned daemon child exited during suspension.")
                    }
                    try await DaemonClient.write(.suspend(projectRoot: root, target: .all), to: connection)
                    guard case .suspendResult = DaemonClient.parse(try await reader.readLine()) else {
                        throw DaemonLifecycleError.recoveryRequired(
                            "Daemon did not confirm suspension; some agents may already be suspended.")
                    }
                }
                guard process.isRunning else {
                    throw DaemonLifecycleError.recoveryRequired("Owned daemon child exited before shutdown.")
                }
                try await DaemonClient.write(.shutdown, to: connection)
                guard case .shuttingDown = DaemonClient.parse(try await reader.readLine()) else {
                    throw DaemonLifecycleError.recoveryRequired("Daemon did not confirm shutdown.")
                }
            }
        } catch let error as DaemonLifecycleError { throw error } catch {
            throw DaemonLifecycleError.recoveryRequired("Owned daemon IPC failed: \(error.localizedDescription)")
        }
        while process.isRunning && DispatchTime.now().uptimeNanoseconds < deadline {
            try await Task.sleep(nanoseconds: 25_000_000)
        }
        guard !process.isRunning else {
            throw DaemonLifecycleError.recoveryRequired("Daemon acknowledged shutdown but its child has not exited.")
        }
        launchedProcess = nil
    }

    private func launchDaemon() async throws {
        guard let binaryPath = binaryOverride ?? DaemonLifecycle.findBinary() else {
            throw DaemonLifecycleError.binaryNotFound
        }
        try DaemonStateDirectory.prepare(puDir)
        let process = Process()
        process.executableURL = URL(fileURLWithPath: binaryPath)
        process.arguments = ["--managed", "--socket", socketPath]
        process.standardOutput = FileHandle.nullDevice
        let stderr = try DaemonStateDirectory.openLog(puDir.appendingPathComponent("daemon.log"))
        process.standardError = stderr
        defer { try? stderr.close() }
        try process.run()
        launchedProcess = process

        let deadline = DispatchTime.now().uptimeNanoseconds + UInt64(startupTimeout * 1_000_000_000)
        var delay: UInt64 = 25_000_000
        while DispatchTime.now().uptimeNanoseconds < deadline {
            let beforeSleep = DispatchTime.now().uptimeNanoseconds
            try await Task.sleep(nanoseconds: min(delay, deadline - min(beforeSleep, deadline)))
            let now = DispatchTime.now().uptimeNanoseconds
            guard now < deadline else { break }
            let remaining = Double(deadline - now) / 1_000_000_000
            switch await probe(timeout: min(probeTimeout, remaining)) {
            case .healthy(let pid):
                if process.isRunning && pid == Int(process.processIdentifier) { return }
                // A flock loser exits without ownership. Never discard a still
                // running child to claim that a different endpoint is ours.
                if !process.isRunning { launchedProcess = nil; return }
                throw DaemonLifecycleError.recoveryRequired(
                    "Another daemon answered while the launched child is still running.")
            case .busy: throw DaemonLifecycleError.recoveryRequired("Daemon is busy; startup compatibility is unknown.")
            case .incompatible:
                throw DaemonLifecycleError.recoveryRequired("Daemon protocol is incompatible with this app.")
            case .unreachable: break
            }
            delay = min(delay * 2, 200_000_000)
        }
        throw DaemonLifecycleError.startupTimeout
    }

    private enum Probe {
        case healthy(pid: Int)
        case busy
        case incompatible
        case unreachable
    }

    private func probe(timeout: TimeInterval? = nil) async -> Probe {
        switch try? await DaemonClient(socketPath: socketPath).send(.health, timeout: timeout ?? probeTimeout) {
        case .healthReport(let pid, _, let version, _)?:
            return version == DaemonClient.protocolVersion ? .healthy(pid: pid) : .incompatible
        case .error(let code, _)? where code == "BUSY": return .busy
        default: return .unreachable
        }
    }
}

nonisolated enum DaemonLifecycleError: Error, LocalizedError {
    case binaryNotFound
    case startupTimeout
    case recoveryRequired(String)

    var errorDescription: String? {
        switch self {
        case .binaryNotFound: "Could not find pu-engine binary. Install it or add it to PATH."
        case .startupTimeout:
            "Daemon startup could not be established within the readiness deadline. Inspect the private daemon log and existing state. No existing daemon was signaled or replaced. Save agent work and recover deliberately before retrying."
        case .recoveryRequired(let reason):
            "\(reason) No OS signal was sent. Save agent work and inspect the existing daemon and private state before deliberate recovery."
        }
    }
}
