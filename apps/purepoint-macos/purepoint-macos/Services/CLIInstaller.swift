import Foundation

enum CLIInstaller {
    private static let installDir = FileManager.default.homeDirectoryForCurrentUser
        .appendingPathComponent(".pu/bin")
    private static let pluginDir = FileManager.default.homeDirectoryForCurrentUser
        .appendingPathComponent(".claude/plugins/purepoint")
    private static let oldSkillDir = FileManager.default.homeDirectoryForCurrentUser
        .appendingPathComponent(".claude/skills/pu")

    /// Copy the bundled `pu` binary to ~/.pu/bin/pu and plugin to ~/.claude/plugins/purepoint/
    /// if they're newer or missing.
    static func installIfNeeded() {
        guard let macosDir = Bundle.main.executableURL?.deletingLastPathComponent() else { return }

        installBinary(from: macosDir)
        installPlugin(from: macosDir)
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

    private static func installPlugin(from macosDir: URL) {
        let bundledPlugin =
            macosDir
            .deletingLastPathComponent()
            .appendingPathComponent("Resources/pu-plugin")
        guard FileManager.default.fileExists(atPath: bundledPlugin.path) else { return }

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
            return
        }

        // Remove old install and copy fresh
        try? FileManager.default.removeItem(at: pluginDir)
        try? FileManager.default.createDirectory(
            at: pluginDir.deletingLastPathComponent(),
            withIntermediateDirectories: true
        )
        try? FileManager.default.copyItem(at: bundledPlugin, to: pluginDir)

        // Migrate: remove old skill location
        if FileManager.default.fileExists(atPath: oldSkillDir.path) {
            try? FileManager.default.removeItem(at: oldSkillDir)
        }
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
