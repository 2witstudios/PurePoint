import Foundation

nonisolated enum DaemonLifecycle {
    private static let launcher = DaemonLauncher()

    /// Ensure the daemon is running. If not, start it and wait for readiness.
    static func ensureDaemon() async throws {
        try await launcher.ensureDaemon()
    }

    /// Restart the daemon: kill existing, then launch fresh.
    static func restartDaemon() async throws {
        try await launcher.restartDaemon()
    }

    /// True if this app instance launched the daemon it is talking to.
    /// Used to avoid shutting down a daemon we merely attached to (e.g. a
    /// second app instance quitting must not kill the shared daemon).
    static func didLaunchDaemon() async -> Bool {
        await launcher.didLaunchDaemon
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
private actor DaemonLauncher {
    private let puDir = FileManager.default.homeDirectoryForCurrentUser
        .appendingPathComponent(".pu")
    private var pidPath: String { puDir.appendingPathComponent("daemon.pid").path }
    private(set) var didLaunchDaemon = false
    private var inFlight: Task<Void, Error>?

    func ensureDaemon() async throws {
        try await serialized(coalesce: true) { try await $0.ensureDaemonNow() }
    }

    func restartDaemon() async throws {
        try await serialized(coalesce: false) { try await $0.restartDaemonNow() }
    }

    /// Run `operation` as the only lifecycle operation in flight. With `coalesce`,
    /// a caller arriving while another operation runs just awaits that one.
    private func serialized(
        coalesce: Bool,
        _ operation: @escaping @Sendable (DaemonLauncher) async throws -> Void
    ) async throws {
        while let running = inFlight {
            let outcome = await running.result
            if coalesce { return try outcome.get() }
        }
        let task = Task { try await operation(self) }
        inFlight = task
        defer { if inFlight == task { inFlight = nil } }
        try await task.value
    }

    private func ensureDaemonNow() async throws {
        let binaryPath = DaemonLifecycle.findBinary()

        let healthy = await isHealthy(attempts: 3)
        let restart = healthy && shouldRestart(binaryPath: binaryPath)
        print("[Daemon] healthy=\(healthy), shouldRestart=\(restart)")
        if healthy && !restart { return }

        await killExistingDaemon()
        try await launchDaemon(binaryPath: binaryPath)
    }

    private func restartDaemonNow() async throws {
        await killExistingDaemon()
        try await launchDaemon(binaryPath: DaemonLifecycle.findBinary())
    }

    // MARK: - Private

    /// Stop the daemon named by the PID file. The socket is left alone: only the
    /// daemon holding `daemon.lock` may unlink or rebind it, so deleting it here
    /// could strand a live daemon that is merely slow to answer.
    private func killExistingDaemon() async {
        guard let content = try? String(contentsOfFile: pidPath, encoding: .utf8),
            let pid = pid_t(content.trimmingCharacters(in: .whitespacesAndNewlines)),
            pid > 0,
            kill(pid, 0) == 0
        else {
            try? FileManager.default.removeItem(atPath: pidPath)
            return
        }

        // SIGTERM (IPC shutdown removed — fire-and-forget raced with the signal)
        kill(pid, SIGTERM)

        // Poll for death (up to 3s, 100ms intervals)
        for _ in 0..<30 {
            try? await Task.sleep(nanoseconds: 100_000_000)
            if kill(pid, 0) != 0 { return }
        }

        kill(pid, SIGKILL)
        try? await Task.sleep(nanoseconds: 200_000_000)
    }

    private func launchDaemon(binaryPath: String?) async throws {
        guard let binaryPath else {
            throw DaemonLifecycleError.binaryNotFound
        }
        print("[Daemon] launching: \(binaryPath)")

        let process = Process()
        process.executableURL = URL(fileURLWithPath: binaryPath)
        process.arguments = ["--managed"]
        process.standardOutput = FileHandle.nullDevice

        // Redirect stderr to log file for diagnostics
        try? FileManager.default.createDirectory(at: puDir, withIntermediateDirectories: true)
        let logFile = puDir.appendingPathComponent("daemon.log")
        process.standardError =
            FileHandle(forWritingAtPath: logFile.path)
            ?? {
                FileManager.default.createFile(atPath: logFile.path, contents: nil)
                return FileHandle(forWritingAtPath: logFile.path) ?? FileHandle.nullDevice
            }()

        try process.run()

        // Close stderr FileHandle in parent — the child has its own copy
        if let stderrHandle = process.standardError as? FileHandle,
            stderrHandle !== FileHandle.nullDevice
        {
            try? stderrHandle.close()
        }

        // Poll health with backoff: 100ms, 200ms, 400ms, 800ms, 1600ms (total ~3s).
        // If another pu-engine already holds the daemon lock, ours exits at once
        // and this attaches to the running one.
        for attempt in 0..<5 {
            let delay = UInt64(100_000_000 * (1 << attempt))
            try await Task.sleep(nanoseconds: delay)
            if await isHealthy(attempts: 1) {
                didLaunchDaemon = true
                return
            }
        }

        throw DaemonLifecycleError.startupTimeout
    }

    /// Returns true if the app bundle's pu-engine binary is newer than the PID file.
    private func shouldRestart(binaryPath: String?) -> Bool {
        guard let binaryPath else { return false }

        guard let binaryDate = modDate(path: binaryPath),
            let pidDate = modDate(path: pidPath)
        else {
            print("[Daemon] shouldRestart: missing dates for binary=\(binaryPath) or pid=\(pidPath)")
            return false
        }

        let result = binaryDate > pidDate
        print("[Daemon] binary=\(binaryPath) (\(binaryDate)), pid=\(pidDate), restart=\(result)")
        return result
    }

    private func modDate(path: String) -> Date? {
        try? FileManager.default.attributesOfItem(atPath: path)[.modificationDate] as? Date
    }

    /// Health-check the daemon, retrying a failed check so one slow answer from
    /// a busy daemon is not mistaken for a dead one (which would kill its agents).
    private func isHealthy(attempts: Int) async -> Bool {
        for attempt in 0..<attempts {
            if attempt > 0 { try? await Task.sleep(nanoseconds: 250_000_000) }
            if await healthCheck(timeout: 2.0) { return true }
        }
        return false
    }

    private func healthCheck(timeout: TimeInterval) async -> Bool {
        await withTaskGroup(of: Bool.self) { group in
            group.addTask {
                guard let response = try? await DaemonClient().send(.health) else { return false }
                if case .healthReport = response { return true }
                return false
            }
            group.addTask {
                try? await Task.sleep(nanoseconds: UInt64(timeout * 1_000_000_000))
                return false
            }
            let first = await group.next() ?? false
            group.cancelAll()
            return first
        }
    }
}

enum DaemonLifecycleError: Error, LocalizedError {
    case binaryNotFound
    case startupTimeout

    var errorDescription: String? {
        switch self {
        case .binaryNotFound: "Could not find pu-engine binary. Install it or add it to PATH."
        case .startupTimeout: "Daemon did not become healthy within 3 seconds."
        }
    }
}
