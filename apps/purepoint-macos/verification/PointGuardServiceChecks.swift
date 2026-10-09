import Foundation

@main struct PointGuardServiceChecks {
    static func main() throws {
        let base = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: base) }
        let resources = base.appendingPathComponent("Contents/Resources/PointGuard")
        let helpers = base.appendingPathComponent("Contents/Helpers")
        try FileManager.default.createDirectory(at: resources, withIntermediateDirectories: true)
        try FileManager.default.createDirectory(at: helpers, withIntermediateDirectories: true)
        let manifest = """
        {"schemaVersion":1,"contractVersion":1,"piVersion":"1.1.0","nodeVersion":"22.23.0","architecture":"arm64","sourceSHA":"fixture","paths":{"node":"../../Helpers/point-guard-node","pu":"../../Helpers/pu","entry":"bridge/main.js","instructions":"docs/point-guard.md","skills":["support/pu/SKILL.md","support/pu-cli/SKILL.md"]}}
        """
        try FileManager.default.createDirectory(at: resources.appendingPathComponent("bridge"), withIntermediateDirectories: true)
        try Data().write(to: resources.appendingPathComponent("bridge/main.js"))
        for name in ["point-guard-node", "pu"] {
            let file = helpers.appendingPathComponent(name)
            try Data("#!/bin/sh\n".utf8).write(to: file)
            try FileManager.default.setAttributes([.posixPermissions: 0o700], ofItemAtPath: file.path)
        }
        for relative in ["docs/point-guard.md", "support/pu/SKILL.md", "support/pu-cli/SKILL.md"] {
            let url = resources.appendingPathComponent(relative)
            try FileManager.default.createDirectory(at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
            try Data().write(to: url)
        }
        let file = resources.appendingPathComponent("runtime-manifest.json")
        try Data(manifest.utf8).write(to: file)
        let runtime = try PointGuardRuntime.load(manifestURL: file)
        precondition(runtime.node.path == helpers.appendingPathComponent("point-guard-node").path)
        // Given relocated resources, bundle-relative helpers should resolve without PATH.
        let escaped = manifest.replacingOccurrences(of: "../../Helpers/pu", with: "../../../outside")
        try Data(escaped.utf8).write(to: file)
        do { _ = try PointGuardRuntime.load(manifestURL: file); fatalError("escaped bundle path accepted") }
        catch { }
        // Given a descriptor for another process or launch, it must never be adopted.
        let descriptor = PointGuardDescriptor(schemaVersion: 1, contractVersion: 1, pid: 42, instanceId: "owned", adminURL: "http://127.0.0.1:1234/admin/v1", nativeChatURL: "ws://127.0.0.1:4321/v1", chatURL: nil, desktopClientId: UUID().uuidString, hostId: UUID().uuidString, certificateSHA256: String(repeating: "a", count: 64))
        precondition(descriptor.belongs(to: 42, instanceId: "owned"))
        precondition(!descriptor.belongs(to: 43, instanceId: "owned"))
        precondition(!descriptor.belongs(to: 42, instanceId: "stale"))
        print("Point Guard runtime path and owned readiness checks passed")
    }
}
