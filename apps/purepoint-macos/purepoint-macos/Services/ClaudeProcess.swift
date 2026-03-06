import Foundation

actor ClaudeProcess: ClaudeProcessProvider {
    private var process: Process?
    private var continuation: AsyncStream<StreamEvent>.Continuation?

    nonisolated func start(prompt: String, cwd: String, sessionId: String?) async throws -> AsyncStream<StreamEvent> {
        try await launch(args: buildArgs(prompt: prompt, cwd: cwd, sessionId: sessionId))
    }

    nonisolated func resume(sessionId: String, prompt: String, cwd: String) async throws -> AsyncStream<StreamEvent> {
        var args = buildArgs(prompt: prompt, cwd: cwd, sessionId: nil)
        args.insert(contentsOf: ["--resume", sessionId], at: 0)
        return try await launch(args: args)
    }

    func cancel() {
        continuation?.finish()
        continuation = nil

        guard let process, process.isRunning else { return }
        process.terminate()

        // Force kill after 3 seconds if still running
        let pid = process.processIdentifier
        DispatchQueue.global().asyncAfter(deadline: .now() + 3) {
            kill(pid, SIGKILL)
        }
        self.process = nil
    }

    static func locateBinary() -> String? {
        let candidates = [
            "\(NSHomeDirectory())/.pu/bin/claude",
            "\(NSHomeDirectory())/.local/bin/claude",
            "/usr/local/bin/claude",
            "/opt/homebrew/bin/claude"
        ]

        for path in candidates {
            if FileManager.default.isExecutableFile(atPath: path) {
                return path
            }
        }

        // Try `which`
        let whichProcess = Process()
        whichProcess.executableURL = URL(fileURLWithPath: "/usr/bin/which")
        whichProcess.arguments = ["claude"]
        let pipe = Pipe()
        whichProcess.standardOutput = pipe
        whichProcess.standardError = FileHandle.nullDevice
        try? whichProcess.run()
        whichProcess.waitUntilExit()
        if whichProcess.terminationStatus == 0 {
            let data = pipe.fileHandleForReading.readDataToEndOfFile()
            let path = String(decoding: data, as: UTF8.self).trimmingCharacters(in: .whitespacesAndNewlines)
            if !path.isEmpty && FileManager.default.isExecutableFile(atPath: path) {
                return path
            }
        }

        return nil
    }

    // MARK: - Private

    private nonisolated func buildArgs(prompt: String, cwd: String, sessionId: String?) -> [String] {
        var args = [
            "-p", prompt,
            "--output-format", "stream-json",
            "--verbose",
            "--cwd", cwd
        ]
        if let sessionId {
            args.append(contentsOf: ["--session-id", sessionId])
        }
        return args
    }

    private func launch(args: [String]) throws -> AsyncStream<StreamEvent> {
        // Cancel any existing process before launching a new one
        if process != nil {
            cancel()
        }

        guard let binaryPath = Self.locateBinary() else {
            throw ClaudeProcessError.binaryNotFound
        }

        let process = Process()
        process.executableURL = URL(fileURLWithPath: binaryPath)
        process.arguments = args
        // Remove CLAUDECODE env var to avoid nested session detection
        var env = ProcessInfo.processInfo.environment
        env.removeValue(forKey: "CLAUDECODE")
        process.environment = env

        let stdoutPipe = Pipe()
        let stderrPipe = Pipe()
        process.standardOutput = stdoutPipe
        process.standardError = stderrPipe

        self.process = process

        let (stream, continuation) = AsyncStream<StreamEvent>.makeStream()
        self.continuation = continuation

        let fileHandle = stdoutPipe.fileHandleForReading

        // Start process BEFORE reader to avoid EOF on not-yet-connected pipe
        try process.run()

        Task.detached { [weak self] in
            for try await line in fileHandle.bytes.lines {
                if let event = StreamEvent.parse(line) {
                    continuation.yield(event)
                }
            }
            continuation.finish()
            await self?.cleanup()
        }

        return stream
    }

    private func cleanup() {
        if let process, process.isRunning {
            process.terminate()
        }
        self.process = nil
        self.continuation = nil
    }
}

enum ClaudeProcessError: Error, LocalizedError {
    case binaryNotFound

    var errorDescription: String? {
        switch self {
        case .binaryNotFound:
            "Claude CLI binary not found. Install Claude Code or check your PATH."
        }
    }
}
