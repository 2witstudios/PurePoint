import Foundation

enum CLIInstaller {
    private static let installDir = FileManager.default.homeDirectoryForCurrentUser
        .appendingPathComponent(".pu/bin")
    private static let skillDir = FileManager.default.homeDirectoryForCurrentUser
        .appendingPathComponent(".claude/skills/pu")

    /// Copy the bundled `pu` binary to ~/.pu/bin/pu and skill to ~/.claude/skills/pu/SKILL.md
    /// if they're newer or missing.
    static func installIfNeeded() {
        guard let macosDir = Bundle.main.executableURL?.deletingLastPathComponent() else { return }

        installBinary(from: macosDir)
        installSkill(from: macosDir)
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

    private static func installSkill(from macosDir: URL) {
        let bundled =
            macosDir
            .deletingLastPathComponent()
            .appendingPathComponent("Resources/pu-skill.md")
        guard FileManager.default.fileExists(atPath: bundled.path) else { return }

        let target = skillDir.appendingPathComponent("SKILL.md")

        if FileManager.default.fileExists(atPath: target.path),
            isUpToDate(source: bundled, target: target)
        {
            return
        }

        try? FileManager.default.createDirectory(at: skillDir, withIntermediateDirectories: true)
        try? FileManager.default.removeItem(at: target)
        try? FileManager.default.copyItem(at: bundled, to: target)
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
