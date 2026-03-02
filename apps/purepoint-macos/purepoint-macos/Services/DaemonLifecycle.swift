import Foundation

enum DaemonLifecycle {
    /// Ensure the daemon is running. If not, start it and wait for readiness.
    static func ensureDaemon() async throws {
        let client = DaemonClient()

        // Check if already healthy
        if await isHealthy(client: client) { return }

        // Find the pu-engine binary
        guard let binaryPath = findBinary() else {
            throw DaemonLifecycleError.binaryNotFound
        }

        // Launch daemon
        let process = Process()
        process.executableURL = URL(fileURLWithPath: binaryPath)
        process.arguments = ["--managed"]
        process.standardOutput = FileHandle.nullDevice

        // Redirect stderr to log file for diagnostics
        let logDir = FileManager.default.homeDirectoryForCurrentUser.appendingPathComponent(".pu")
        try? FileManager.default.createDirectory(at: logDir, withIntermediateDirectories: true)
        let logFile = logDir.appendingPathComponent("daemon.log")
        process.standardError = FileHandle(forWritingAtPath: logFile.path) ?? {
            FileManager.default.createFile(atPath: logFile.path, contents: nil)
            return FileHandle(forWritingAtPath: logFile.path) ?? FileHandle.nullDevice
        }()

        // Remove stale socket so new daemon can bind
        let socketPath = FileManager.default.homeDirectoryForCurrentUser
            .appendingPathComponent(".pu/daemon.sock").path
        try? FileManager.default.removeItem(atPath: socketPath)

        try process.run()

        // Poll health with backoff: 100ms, 200ms, 400ms, 800ms, 1600ms (total ~3s)
        for attempt in 0..<5 {
            let delay = UInt64(100_000_000 * (1 << attempt)) // nanoseconds
            try await Task.sleep(nanoseconds: delay)
            if await isHealthy(client: client) { return }
        }

        throw DaemonLifecycleError.startupTimeout
    }

    private static func isHealthy(client: DaemonClient) async -> Bool {
        do {
            let response = try await client.send(.health)
            if case .healthReport = response { return true }
            return false
        } catch {
            return false
        }
    }

    private static func findBinary() -> String? {
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
