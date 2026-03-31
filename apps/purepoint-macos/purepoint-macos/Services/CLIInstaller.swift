import Foundation
import os.log

private let logger = Logger(subsystem: "com.purepoint.macos", category: "CLIInstaller")

enum CLIInstaller {
    private static let home = FileManager.default.homeDirectoryForCurrentUser
    private static let installDir = home.appendingPathComponent(".pu/bin")
    private static let marketplaceDir = home.appendingPathComponent(".pu/marketplace")
    private static let pluginDir = marketplaceDir.appendingPathComponent("plugins/pu")
    private static let oldPluginDir = home.appendingPathComponent(".claude/plugins/purepoint")
    private static let oldSkillDir = home.appendingPathComponent(".claude/skills/pu")

    /// Copy the bundled `pu` binary to ~/.pu/bin/pu, set up the local marketplace
    /// at ~/.pu/marketplace/, and register it with Claude Code if needed.
    static func installIfNeeded() {
        guard let macosDir = Bundle.main.executableURL?.deletingLastPathComponent() else { return }

        installBinary(from: macosDir)
        let pluginUpdated = installPlugin(from: macosDir)
        if pluginUpdated {
            DispatchQueue.global().async {
                registerWithClaudeCode()
            }
        }
    }

    private static func installBinary(from macosDir: URL) {
        let bundled = macosDir.appendingPathComponent("pu")
        guard FileManager.default.isExecutableFile(atPath: bundled.path) else { return }

        let target = installDir.appendingPathComponent("pu")

        if FileManager.default.isExecutableFile(atPath: target.path),
            isUpToDate(source: bundled, target: target)
        {
            return
        }

        try? FileManager.default.createDirectory(at: installDir, withIntermediateDirectories: true)
        try? FileManager.default.removeItem(at: target)
        try? FileManager.default.copyItem(at: bundled, to: target)
    }

    /// Copy bundled plugin into the local marketplace at ~/.pu/marketplace/plugins/pu/
    /// and write the marketplace manifest. Returns true if files were updated.
    private static func installPlugin(from macosDir: URL) -> Bool {
        let bundledPlugin =
            macosDir
            .deletingLastPathComponent()
            .appendingPathComponent("Resources/pu-plugin")
        guard FileManager.default.fileExists(atPath: bundledPlugin.path) else { return false }

        // Use plugin.json as the freshness sentinel
        let bundledSentinel =
            bundledPlugin
            .appendingPathComponent(".claude-plugin/plugin.json")
        let targetSentinel =
            pluginDir
            .appendingPathComponent(".claude-plugin/plugin.json")

        if FileManager.default.fileExists(atPath: targetSentinel.path),
            isUpToDate(source: bundledSentinel, target: targetSentinel)
        {
            return false
        }

        // Copy plugin into marketplace directory
        try? FileManager.default.removeItem(at: pluginDir)
        try? FileManager.default.createDirectory(
            at: pluginDir.deletingLastPathComponent(),
            withIntermediateDirectories: true
        )
        try? FileManager.default.copyItem(at: bundledPlugin, to: pluginDir)

        // Write marketplace manifest
        writeMarketplaceManifest()

        // Migrate: remove old install locations
        if FileManager.default.fileExists(atPath: oldPluginDir.path) {
            try? FileManager.default.removeItem(at: oldPluginDir)
        }
        if FileManager.default.fileExists(atPath: oldSkillDir.path) {
            try? FileManager.default.removeItem(at: oldSkillDir)
        }

        return true
    }

    private static func writeMarketplaceManifest() {
        let manifestDir = marketplaceDir.appendingPathComponent(".claude-plugin")
        let manifestFile = manifestDir.appendingPathComponent("marketplace.json")

        try? FileManager.default.createDirectory(
            at: manifestDir, withIntermediateDirectories: true
        )

        let manifest = """
            {
              "$schema": "https://anthropic.com/claude-code/marketplace.schema.json",
              "name": "purepoint",
              "version": "0.1.0",
              "description": "PurePoint agent orchestration plugin",
              "owner": { "name": "PurePoint" },
              "plugins": [
                {
                  "name": "pu",
                  "source": "./plugins/pu",
                  "category": "development",
                  "description": "Spawn parallel AI agents in isolated worktrees, manage long-horizon tasks, and coordinate agent teams."
                }
              ]
            }
            """
        try? manifest.data(using: .utf8)?.write(to: manifestFile)
    }

    /// Register the local marketplace and install the plugin via `claude` CLI.
    private static func registerWithClaudeCode() {
        guard let claudePath = findClaude() else {
            logger.warning("Claude Code CLI not found — skipping plugin registration")
            return
        }
        let mktPath = marketplaceDir.path

        // Add marketplace (idempotent — re-adding an existing one is fine)
        if !run(claudePath, "plugins", "marketplace", "add", mktPath) {
            logger.error("Failed to add marketplace at \(mktPath)")
        }

        // Install the plugin (idempotent — installing an already-installed plugin is fine)
        if !run(claudePath, "plugins", "install", "pu@purepoint") {
            logger.error("Failed to install pu@purepoint plugin")
        }
    }

    private static func findClaude() -> String? {
        let candidates = [
            "/usr/local/bin/claude",
            home.appendingPathComponent(".claude/local/claude").path,
            "/opt/homebrew/bin/claude",
        ]
        for path in candidates {
            if FileManager.default.isExecutableFile(atPath: path) {
                return path
            }
        }
        // Fall back to PATH lookup
        let result = Process()
        result.executableURL = URL(fileURLWithPath: "/usr/bin/which")
        result.arguments = ["claude"]
        let pipe = Pipe()
        result.standardOutput = pipe
        result.standardError = FileHandle.nullDevice
        try? result.run()
        result.waitUntilExit()
        let data = pipe.fileHandleForReading.readDataToEndOfFile()
        guard result.terminationStatus == 0,
            let path = String(data: data, encoding: .utf8)?.trimmingCharacters(in: .whitespacesAndNewlines),
            !path.isEmpty
        else { return nil }
        return path
    }

    @discardableResult
    private static func run(_ args: String...) -> Bool {
        let process = Process()
        process.executableURL = URL(fileURLWithPath: args[0])
        process.arguments = Array(args.dropFirst())
        process.standardOutput = FileHandle.nullDevice
        process.standardError = FileHandle.nullDevice
        // Ensure PATH includes common locations
        var env = ProcessInfo.processInfo.environment
        env["PATH"] = (env["PATH"] ?? "") + ":/usr/local/bin:/opt/homebrew/bin"
        process.environment = env
        try? process.run()
        process.waitUntilExit()
        return process.terminationStatus == 0
    }

    private static func isUpToDate(source: URL, target: URL) -> Bool {
        guard let sourceAttrs = try? FileManager.default.attributesOfItem(atPath: source.path),
            let targetAttrs = try? FileManager.default.attributesOfItem(atPath: target.path),
            let sourceDate = sourceAttrs[.modificationDate] as? Date,
            let targetDate = targetAttrs[.modificationDate] as? Date
        else { return false }
        return targetDate >= sourceDate
    }
}
