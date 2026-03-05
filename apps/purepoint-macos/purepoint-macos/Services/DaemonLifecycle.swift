import Foundation

enum DaemonLifecycle {
    private static let launcher = DaemonLauncher()

    /// Ensure the daemon is running. If not, start it and wait for readiness.
    static func ensureDaemon() async throws {
        try await launcher.ensureDaemon()
    }

    /// Restart the daemon: kill existing, then launch fresh.
    static func restartDaemon() async throws {
        try await launcher.restartDaemon()
    }

    static func findBinary() -> String? {
        // 1. Check app bundle (production path)
        if let bundlePath = Bundle.main.executableURL?
            .deletingLastPathComponent()
            .appendingPathComponent("pu-engine").path,
           FileManager.default.isExecutableFile(atPath: bundlePath) {
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
private actor DaemonLauncher {
    private let puDir = FileManager.default.homeDirectoryForCurrentUser
        .appendingPathComponent(".pu")
    private var pidPath: String { puDir.appendingPathComponent("daemon.pid").path }
    private var socketPath: String { puDir.appendingPathComponent("daemon.sock").path }

    func ensureDaemon() async throws {
        let client = DaemonClient()

        // Check if already healthy
        if await isHealthy(client: client) {
            if shouldRestart() {
                killExistingDaemon()
            } else {
                return
            }
        }

        try await launchDaemon()
    }

    func restartDaemon() async throws {
        killExistingDaemon()
        try await launchDaemon()
    }

    // MARK: - Private

    private func killExistingDaemon() {
        // Read PID from file
        guard let content = try? String(contentsOfFile: pidPath, encoding: .utf8),
              let pid = pid_t(content.trimmingCharacters(in: .whitespacesAndNewlines)),
              pid > 0,
              kill(pid, 0) == 0 else {
            // No running process — just clean up stale files
            cleanupFiles()
            return
        }

        // SIGTERM (IPC shutdown removed — fire-and-forget raced with the signal)
        kill(pid, SIGTERM)

        // Poll for death (up to 2s, 100ms intervals)
        for _ in 0..<20 {
            Thread.sleep(forTimeInterval: 0.1)
            if kill(pid, 0) != 0 { break }
        }

        // Force kill if still alive
        if kill(pid, 0) == 0 {
            kill(pid, SIGKILL)
            Thread.sleep(forTimeInterval: 0.1)
        }

        cleanupFiles()
    }

    private func cleanupFiles() {
        try? FileManager.default.removeItem(atPath: pidPath)
        try? FileManager.default.removeItem(atPath: socketPath)
    }

    private func launchDaemon() async throws {
        guard let binaryPath = DaemonLifecycle.findBinary() else {
            throw DaemonLifecycleError.binaryNotFound
        }

        let process = Process()
        process.executableURL = URL(fileURLWithPath: binaryPath)
        process.arguments = []
        process.standardOutput = FileHandle.nullDevice

        // Redirect stderr to log file for diagnostics
        try? FileManager.default.createDirectory(at: puDir, withIntermediateDirectories: true)
        let logFile = puDir.appendingPathComponent("daemon.log")
        process.standardError = FileHandle(forWritingAtPath: logFile.path) ?? {
            FileManager.default.createFile(atPath: logFile.path, contents: nil)
            return FileHandle(forWritingAtPath: logFile.path) ?? FileHandle.nullDevice
        }()

        // Clean up any stale files before launching
        cleanupFiles()

        try process.run()

        // Poll health with backoff: 100ms, 200ms, 400ms, 800ms, 1600ms (total ~3s)
        let client = DaemonClient()
        for attempt in 0..<5 {
            let delay = UInt64(100_000_000 * (1 << attempt))
            try await Task.sleep(nanoseconds: delay)
            if await isHealthy(client: client) { return }
        }

        throw DaemonLifecycleError.startupTimeout
    }

    /// Returns true if the app bundle's pu-engine binary is newer than the PID file.
    private func shouldRestart() -> Bool {
        guard let binaryPath = DaemonLifecycle.findBinary() else { return false }

        guard let binaryDate = modDate(path: binaryPath),
              let pidDate = modDate(path: pidPath) else { return false }

        return binaryDate > pidDate
    }

    private func modDate(path: String) -> Date? {
        try? FileManager.default.attributesOfItem(atPath: path)[.modificationDate] as? Date
    }

    private func isHealthy(client: DaemonClient) async -> Bool {
        do {
            let response = try await client.send(.health)
            if case .healthReport = response { return true }
            return false
        } catch {
            return false
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
