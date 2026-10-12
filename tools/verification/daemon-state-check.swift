import Foundation

@main
struct DaemonStateCheck {
    static func main() throws {
        let root = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: false)
        precondition(chmod(root.path, 0o700) == 0)
        defer { try? FileManager.default.removeItem(at: root) }
        let unsafeAncestor = root.appendingPathComponent("public-parent")
        precondition(mkdir(unsafeAncestor.path, 0o777) == 0)
        precondition(chmod(unsafeAncestor.path, 0o777) == 0)
        let unsafeChild = unsafeAncestor.appendingPathComponent("private")
        precondition(mkdir(unsafeChild.path, 0o700) == 0)
        do {
            _ = try DaemonStateDirectory.openLog(unsafeChild.appendingPathComponent("daemon.log"))
            preconditionFailure("log created through public ancestor")
        } catch {}
        precondition(!FileManager.default.fileExists(atPath: unsafeChild.appendingPathComponent("daemon.log").path))
        let missingChild = unsafeAncestor.appendingPathComponent("missing")
        do {
            try DaemonStateDirectory.prepare(missingChild)
            preconditionFailure("created state through public ancestor")
        } catch {}
        precondition(!FileManager.default.fileExists(atPath: missingChild.path))
        let state = root.appendingPathComponent("state")
        try DaemonStateDirectory.prepare(state)
        var entry = stat()
        precondition(lstat(state.path, &entry) == 0)
        precondition(entry.st_mode & 0o7777 == 0o700)
        precondition(chmod(state.path, 0o755) == 0)
        do {
            try DaemonStateDirectory.prepare(state)
            preconditionFailure("public existing state accepted")
        } catch {}
        precondition(lstat(state.path, &entry) == 0 && entry.st_mode & 0o7777 == 0o755)
        precondition(chmod(state.path, 0o700) == 0)
        let safeLog = state.appendingPathComponent("safe.log")
        let first = try DaemonStateDirectory.openLog(safeLog)
        try first.write(contentsOf: Data("one".utf8))
        try first.close()
        let second = try DaemonStateDirectory.openLog(safeLog)
        try second.write(contentsOf: Data("two".utf8))
        try second.close()
        let logContents = try String(contentsOf: safeLog, encoding: .utf8)
        precondition(logContents == "onetwo")
        precondition(lstat(safeLog.path, &entry) == 0 && entry.st_mode & 0o7777 == 0o600)
        for kind in ["public", "link", "hardlink", "fifo", "directory"] {
            let path = state.appendingPathComponent(kind)
            switch kind {
            case "public":
                try Data("preserved".utf8).write(to: path)
                precondition(chmod(path.path, 0o644) == 0)
            case "link": precondition(symlink(safeLog.path, path.path) == 0)
            case "hardlink": precondition(link(safeLog.path, path.path) == 0)
            case "directory": precondition(mkdir(path.path, 0o700) == 0)
            default: precondition(mkfifo(path.path, 0o600) == 0)
            }
            var before = stat()
            precondition(lstat(path.path, &before) == 0)
            do {
                _ = try DaemonStateDirectory.openLog(path)
                preconditionFailure("unsafe log accepted: \(kind)")
            } catch {}
            precondition(lstat(path.path, &entry) == 0)
            precondition(
                entry.st_ino == before.st_ino && entry.st_mode == before.st_mode && entry.st_size == before.st_size)
            let preservedContents = try String(contentsOf: safeLog, encoding: .utf8)
            precondition(preservedContents == "onetwo")
        }
        func acl(_ rule: String, _ path: URL) throws {
            let command = Process()
            command.executableURL = URL(fileURLWithPath: "/bin/chmod")
            command.arguments = ["+a", rule, path.path]
            try command.run()
            command.waitUntilExit()
            precondition(command.terminationStatus == 0)
        }
        try acl("everyone deny delete", root)
        try DaemonStateDirectory.prepare(state)
        let aclLog = state.appendingPathComponent("acl.log")
        let aclHandle = try DaemonStateDirectory.openLog(aclLog)
        try aclHandle.write(contentsOf: Data("preserved".utf8))
        try aclHandle.close()
        try acl("everyone allow read", aclLog)
        do {
            _ = try DaemonStateDirectory.openLog(aclLog)
            preconditionFailure("grant ACL log accepted")
        } catch {}
        let aclContent = try String(contentsOf: aclLog, encoding: .utf8)
        precondition(aclContent == "preserved")
        try acl("everyone allow search", root)
        do {
            _ = try DaemonStateDirectory.openLog(state.appendingPathComponent("missing.log"))
            preconditionFailure("grant ACL ancestor accepted")
        } catch {}
        precondition(!FileManager.default.fileExists(atPath: state.appendingPathComponent("missing.log").path))
        do {
            try DaemonStateDirectory.prepare(root.appendingPathComponent("missing"))
            preconditionFailure("state created under grant ACL ancestor")
        } catch {}
        precondition(!FileManager.default.fileExists(atPath: root.appendingPathComponent("missing").path))
        let link = root.appendingPathComponent("alias")
        precondition(symlink(state.path, link.path) == 0)
        do {
            try DaemonStateDirectory.prepare(link)
            preconditionFailure("symlink state accepted")
        } catch {}
        print("PASS private app state and log creation; unsafe modes/types/links preserved and rejected")
    }
}
