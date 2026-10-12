import Foundation

@main
struct DaemonStateCheck {
    static func main() throws {
        let root = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: false)
        defer { try? FileManager.default.removeItem(at: root) }
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
        let link = root.appendingPathComponent("alias")
        precondition(symlink(state.path, link.path) == 0)
        do {
            try DaemonStateDirectory.prepare(link)
            preconditionFailure("symlink state accepted")
        } catch {}
        print("PASS private app state creation; unsafe existing modes and symlinks preserved/rejected")
    }
}
