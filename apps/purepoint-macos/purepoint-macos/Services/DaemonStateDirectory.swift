import Foundation

/// Bootstrap only. The daemon validates the complete ancestor/ACL boundary
/// before opening its control files. Existing state is never chmod/chowned here.
nonisolated enum DaemonStateDirectory {
    static func prepare(_ directory: URL) throws {
        if mkdir(directory.path, 0o700) != 0 && errno != EEXIST {
            throw NSError(domain: NSPOSIXErrorDomain, code: Int(errno))
        }
        var entry = stat()
        guard lstat(directory.path, &entry) == 0 else {
            throw NSError(domain: NSPOSIXErrorDomain, code: Int(errno))
        }
        guard entry.st_mode & S_IFMT == S_IFDIR,
            entry.st_uid == geteuid(), entry.st_mode & 0o7777 == 0o700
        else {
            throw UnsafeState(path: directory.path)
        }
    }

    private struct UnsafeState: LocalizedError {
        let path: String
        var errorDescription: String? {
            "Unsafe daemon state at \(path). Expected an owned directory with mode 0700. Save agent work and stop the daemon before inspecting ownership, symlinks and ACLs and deliberately migrating old permissions. Existing state was left unchanged."
        }
    }
}
