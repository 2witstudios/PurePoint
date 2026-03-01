import Foundation

struct ProcessResult: Sendable {
    let exitCode: Int32
    let stdout: String
    let stderr: String

    var succeeded: Bool { exitCode == 0 }
}

nonisolated enum ProcessRunner {
    /// Run a shell command and return the result. Blocks the calling thread.
    static func run(executable: String = "/bin/zsh", arguments: [String]) -> ProcessResult {
        let task = Process()
        task.executableURL = URL(fileURLWithPath: executable)
        task.arguments = arguments

        let stdoutPipe = Pipe()
        let stderrPipe = Pipe()
        task.standardOutput = stdoutPipe
        task.standardError = stderrPipe

        do {
            try task.run()
            task.waitUntilExit()
        } catch {
            return ProcessResult(exitCode: -1, stdout: "", stderr: error.localizedDescription)
        }

        let stdout = String(data: stdoutPipe.fileHandleForReading.readDataToEndOfFile(), encoding: .utf8)?
            .trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
        let stderr = String(data: stderrPipe.fileHandleForReading.readDataToEndOfFile(), encoding: .utf8)?
            .trimmingCharacters(in: .whitespacesAndNewlines) ?? ""

        return ProcessResult(exitCode: task.terminationStatus, stdout: stdout, stderr: stderr)
    }

    /// Run a tmux command using the resolved tmux path.
    static func runTmux(_ action: TmuxCommandBuilder.Action) -> ProcessResult {
        let args = TmuxCommandBuilder.shellFragments(for: action)
        let cmd = shellProfileScript(for: "/bin/zsh") + " " + args.joined(separator: " ")
        return run(arguments: ["-c", cmd])
    }
}
