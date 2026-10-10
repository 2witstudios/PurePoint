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
        {"schemaVersion":1,"contractVersion":1,"piVersion":"1.1.0","nodeVersion":"22.23.0","architecture":"arm64","sourceSHA":"fixture","paths":{"node":"../../Helpers/point-guard-node","pu":"../../Helpers/pu","lockHelper":"../../Helpers/point-guard-lock","entry":"bridge/main.js","instructions":"docs/point-guard.md","skills":["support/pu/SKILL.md","support/pu-cli/SKILL.md"]}}
        """
        try FileManager.default.createDirectory(at: resources.appendingPathComponent("bridge"), withIntermediateDirectories: true)
        try Data().write(to: resources.appendingPathComponent("bridge/main.js"))
        for name in ["point-guard-node", "pu", "point-guard-lock"] {
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
        let failure = PointGuardStartupFailure(schemaVersion: 1, pid: 42, instanceId: "owned", code: "identity", message: "Host identity unavailable.", recovery: "Restore private trust state deliberately.")
        precondition(failure.description(pid: 42, instanceId: "owned")?.contains("deliberately") == true)
        precondition(failure.description(pid: 42, instanceId: "stale") == nil)
        precondition(failure.description(pid: 43, instanceId: "owned") == nil)
        let selected = base.appendingPathComponent("selected")
        try FileManager.default.createDirectory(at: selected, withIntermediateDirectories: true)
        _ = try PointGuardWorkingFolder.validated(selected.path)
        try FileManager.default.removeItem(at: selected)
        do { _ = try PointGuardWorkingFolder.validated(selected.path); fatalError("missing folder accepted") } catch { }
        let recovered = try PointGuardWorkingFolder.validated(base.path)
        precondition(recovered.path == base.path)
        precondition(!PointGuardTailnet.candidate(interface: "en0", isUp: true, address: "100.64.1.2"))
        precondition(!PointGuardTailnet.candidate(interface: "utun0", isUp: false, address: "100.64.1.2"))
        precondition(!PointGuardTailnet.candidate(interface: "utunknown", isUp: true, address: "100.64.1.2"))
        precondition(!PointGuardTailnet.candidate(interface: "utun0", isUp: true, address: "192.168.1.2"))
        precondition(PointGuardTailnet.candidate(interface: "utun42", isUp: true, address: "100.127.1.2"))
        precondition(PointGuardTailnet.matchedAddress(candidates: ["100.64.1.2"], reported: "100.64.9.9") == nil)
        precondition(PointGuardTailnet.matchedAddress(candidates: ["100.64.1.2"], reported: "100.64.1.2\n100.64.9.9") == nil)
        precondition(PointGuardTailnet.matchedAddress(candidates: ["100.64.1.2"], reported: "100.64.1.2\n") == "100.64.1.2")
        let queryStarted = Date()
        let heldPipe = PointGuardTailnet.query(executable: URL(fileURLWithPath: "/bin/sh"),
            arguments: ["-c", "printf '100.64.1.2\\n'; /bin/sleep 1 & exit 0"], candidates: ["100.64.1.2"], timeout: 0.2)
        precondition(heldPipe == nil && Date().timeIntervalSince(queryStarted) < 0.8, "Descendant-held stdout must time out without accepting an IP")
        let oversized = PointGuardTailnet.query(executable: URL(fileURLWithPath: "/bin/sh"),
            arguments: ["-c", "i=0; while [ $i -lt 1000 ]; do printf '100.64.1.2\\n'; i=$((i+1)); done"], candidates: ["100.64.1.2"], timeout: 1)
        precondition(oversized == nil, "Oversized query output must not be accepted")
        print("Point Guard runtime path, owned readiness, recovery-folder and error attribution checks passed")
    }
}
