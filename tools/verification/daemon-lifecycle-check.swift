import Foundation

@main
struct DaemonLifecycleCheck {
    static func main() async throws {
        let fixture = CommandLine.arguments[1]
        let root = URL(fileURLWithPath: "/tmp").appendingPathComponent("pu-client-\(UUID().uuidString)")
        precondition(mkdir(root.path, 0o700) == 0)
        var children: [Process] = []
        defer {
            // Fixtures exit themselves. Never send a signal, even on test failure.
            for child in children { child.waitUntilExit() }
            try? FileManager.default.removeItem(at: root)
        }
        func state(_ mode: String) throws -> URL {
            let dir = root.appendingPathComponent(UUID().uuidString)
            precondition(mkdir(dir.path, 0o700) == 0)
            try mode.write(to: dir.appendingPathComponent("mode"), atomically: true, encoding: .utf8)
            return dir
        }
        func launcher(_ dir: URL) -> DaemonLauncher {
            DaemonLauncher(stateDirectory: dir, binaryPath: fixture, probeTimeout: 0.2, startupTimeout: 1.0)
        }
        func external(_ dir: URL) async throws -> Process {
            let child = Process()
            child.executableURL = URL(fileURLWithPath: fixture)
            child.arguments = ["--socket", dir.appendingPathComponent("daemon.sock").path]
            child.standardOutput = FileHandle.nullDevice
            child.standardError = FileHandle.nullDevice
            try child.run()
            children.append(child)
            for _ in 0..<40 {
                if FileManager.default.fileExists(atPath: dir.appendingPathComponent("daemon.sock").path) {
                    return child
                }
                try await Task.sleep(nanoseconds: 25_000_000)
            }
            throw DaemonLifecycleError.startupTimeout
        }
        func requests(_ dir: URL) throws -> [[String: Any]] {
            let text = try String(contentsOf: dir.appendingPathComponent("requests"), encoding: .utf8)
            return try text.split(separator: "\n").map {
                try JSONSerialization.jsonObject(with: Data($0.utf8)) as! [String: Any]
            }
        }
        // Unsafe directory state fails before even probing a healthy endpoint.
        let unsafe = try state("healthy")
        let unsafeChild = try await external(unsafe)
        precondition(chmod(unsafe.path, 0o777) == 0)
        do { try await launcher(unsafe).ensureDaemon(); preconditionFailure("unsafe state accepted") } catch {}
        precondition(unsafeChild.isRunning)
        precondition(!FileManager.default.fileExists(atPath: unsafe.appendingPathComponent("requests").path))
        precondition(!FileManager.default.fileExists(atPath: unsafe.appendingPathComponent("daemon.log").path))
        var unsafeEntry = stat()
        precondition(lstat(unsafe.path, &unsafeEntry) == 0 && unsafeEntry.st_mode & 0o7777 == 0o777)
        // Given shared healthy daemon plus stale/advisory PID metadata, attach
        // must preserve it and deliberate restart must refuse ownership.
        let shared = try state("healthy")
        let sharedChild = try await external(shared)
        try String(sharedChild.processIdentifier).write(
            to: shared.appendingPathComponent("daemon.sock.pid"), atomically: true, encoding: .utf8)
        try FileManager.default.setAttributes(
            [.modificationDate: Date(timeIntervalSince1970: 1)],
            ofItemAtPath: shared.appendingPathComponent("daemon.sock.pid").path)
        let attached = launcher(shared)
        try await attached.ensureDaemon()
        let attachedOwns = await attached.ownsRunningDaemon()
        precondition(!attachedOwns)
        try await attached.shutdownDaemon(projectRoots: ["scratch"])
        do { try await attached.restartDaemon(); preconditionFailure("shared restart accepted") } catch {}
        precondition(sharedChild.isRunning)
        let sharedRows = try requests(shared)
        precondition(sharedRows.allSatisfy { $0["type"] as? String == "health" && $0["signal"] == nil })

        // Given an owned live child, pathname replacement during health must
        // leave suspension and shutdown on the already verified connection.
        let owned = try state("rebind")
        let managed = launcher(owned)
        try await managed.ensureDaemon()
        try await managed.shutdownDaemon(projectRoots: ["scratch-a", "scratch-b"])
        let rows = try requests(owned)
        let mutations = rows.filter { ["suspend", "shutdown"].contains($0["type"] as? String ?? "") }
        precondition(mutations.count == 3)
        precondition(mutations.allSatisfy { $0["replacement"] as? Bool == false })
        let health = rows.last { $0["type"] as? String == "health" }!
        precondition(mutations.allSatisfy { $0["connection"] as? Int == health["connection"] as? Int })
        let managedOwns = await managed.ownsRunningDaemon()
        precondition(!managedOwns)

        // A stale PID naming a healthy fixture on another socket confers no
        // authority even when this app needs to launch a fresh daemon.
        let fresh = try state("healthy")
        try String(sharedChild.processIdentifier).write(
            to: fresh.appendingPathComponent("daemon.sock.pid"), atomically: true, encoding: .utf8)
        let freshLauncher = launcher(fresh)
        try await freshLauncher.ensureDaemon()
        precondition(sharedChild.isRunning)
        let freshOwns = await freshLauncher.ownsRunningDaemon()
        precondition(freshOwns)
        try await freshLauncher.restartDaemon()
        let restartedOwns = await freshLauncher.ownsRunningDaemon()
        precondition(restartedOwns)
        try await freshLauncher.shutdownDaemon(projectRoots: [])
        precondition(sharedChild.isRunning)
        let sharedAfter = try requests(shared)
        precondition(sharedAfter.allSatisfy { $0["type"] as? String == "health" && $0["signal"] == nil })

        // An exited owned Process cannot be revived by a replacement endpoint
        // returning its former numeric PID.
        let exited = try state("healthy")
        let exitedLauncher = launcher(exited)
        try await exitedLauncher.ensureDaemon()
        let client = DaemonClient(socketPath: exited.appendingPathComponent("daemon.sock").path)
        guard case .healthReport(let formerPid, _, _, _) = try await client.send(.health, timeout: 1) else {
            preconditionFailure("fixture health missing")
        }
        _ = try await client.send(.shutdown, timeout: 1)
        try await Task.sleep(nanoseconds: 200_000_000)
        if FileManager.default.fileExists(atPath: exited.appendingPathComponent("daemon.sock").path) {
            try FileManager.default.removeItem(at: exited.appendingPathComponent("daemon.sock"))
        }
        try "spoof".write(to: exited.appendingPathComponent("mode"), atomically: true, encoding: .utf8)
        try String(formerPid).write(to: exited.appendingPathComponent("spoof-pid"), atomically: true, encoding: .utf8)
        let replacement = try await external(exited)
        let exitedOwns = await exitedLauncher.ownsRunningDaemon()
        precondition(!exitedOwns)
        do {
            try await exitedLauncher.shutdownDaemon(projectRoots: ["scratch"]);
            preconditionFailure("exited child accepted")
        } catch {}
        precondition(replacement.isRunning)
        let exitedRows = try requests(exited)
        precondition(exitedRows.filter { $0["type"] as? String == "shutdown" }.count == 1)
        precondition(exitedRows.allSatisfy { $0["type"] as? String != "suspend" && $0["signal"] == nil })

        // Shutdown acknowledgement without actual child exit is uncertainty,
        // never a reason to send SIGKILL or launch a replacement.
        let linger = try state("linger")
        let lingerLauncher = launcher(linger)
        try await lingerLauncher.ensureDaemon()
        let lingerBegan = Date()
        do { try await lingerLauncher.restartDaemon(); preconditionFailure("lingering child replaced") } catch {}
        precondition(Date().timeIntervalSince(lingerBegan) < 2)
        let lingerRows = try requests(linger)
        precondition(lingerRows.filter { $0["type"] as? String == "shutdown" }.count == 1)
        precondition(lingerRows.allSatisfy { $0["signal"] == nil })
        // A different live daemon at the pathname fails ownership before any
        // mutation, even while the original launched child remains alive.
        let replaced = try state("healthy")
        let replacedLauncher = launcher(replaced)
        try await replacedLauncher.ensureDaemon()
        try FileManager.default.removeItem(at: replaced.appendingPathComponent("daemon.sock"))
        let differentChild = try await external(replaced)
        do { try await replacedLauncher.restartDaemon(); preconditionFailure("replacement daemon stopped") } catch {}
        let replacedOwns = await replacedLauncher.ownsRunningDaemon()
        precondition(!replacedOwns && differentChild.isRunning)
        let replacedRows = try requests(replaced)
        precondition(replacedRows.allSatisfy { $0["type"] as? String == "health" && $0["signal"] == nil })

        // An owned child's busy or stalled health cannot authorize shutdown.
        for mode in ["busy", "mute"] {
            let dir = try state("healthy")
            let ownedLauncher = launcher(dir)
            try await ownedLauncher.ensureDaemon()
            try mode.write(to: dir.appendingPathComponent("mode"), atomically: true, encoding: .utf8)
            let began = Date()
            do {
                try await ownedLauncher.restartDaemon(); preconditionFailure("unverified owned shutdown accepted")
            } catch {}
            precondition(Date().timeIntervalSince(began) < 2)
            let rows = try requests(dir)
            precondition(rows.allSatisfy { $0["type"] as? String == "health" && $0["signal"] == nil })
        }
        // BUSY and incompatible endpoints are not startup permission.
        for mode in ["busy", "old"] {
            let dir = try state(mode)
            let child = try await external(dir)
            let client = launcher(dir)
            let began = Date()
            do { try await client.ensureDaemon(); preconditionFailure("\(mode) accepted") } catch {}
            precondition(Date().timeIntervalSince(began) < 2)
            precondition(child.isRunning)
            let errorRows = try requests(dir)
            precondition(errorRows.allSatisfy { $0["type"] as? String == "health" && $0["signal"] == nil })
        }
        // An accepted but mute endpoint is bounded and never signaled.
        let mute = try state("mute")
        let muteChild = try await external(mute)
        let began = Date()
        do { try await launcher(mute).ensureDaemon(); preconditionFailure("mute accepted") } catch {}
        precondition(Date().timeIntervalSince(began) < 3)
        precondition(muteChild.isRunning)
        let muteRows = try requests(mute)
        precondition(muteRows.allSatisfy { $0["type"] as? String == "health" && $0["signal"] == nil })
        print(
            "PASS shared/stale PID preservation, owned shutdown/restart, replacements/exited child, no hard kill, bounded busy/mute"
        )
    }
}
