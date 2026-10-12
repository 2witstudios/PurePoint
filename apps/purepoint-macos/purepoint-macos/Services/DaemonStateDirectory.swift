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

    static func openLog(_ path: URL) throws -> FileHandle {
        func validate(_ entry: stat) throws {
            guard entry.st_mode & S_IFMT == S_IFREG, entry.st_uid == geteuid(),
                entry.st_nlink == 1, entry.st_mode & 0o7777 == 0o600
            else {
                throw UnsafeLog(path: path.path)
            }
        }
        var entry = stat()
        if lstat(path.path, &entry) == 0 {
            try validate(entry)
        } else if errno != ENOENT {
            throw NSError(domain: NSPOSIXErrorDomain, code: Int(errno))
        }
        // No special-file open before validation, and a raced FIFO cannot block.
        let fd = open(path.path, O_WRONLY | O_CREAT | O_APPEND | O_CLOEXEC | O_NOFOLLOW | O_NONBLOCK, 0o600)
        guard fd >= 0 else { throw NSError(domain: NSPOSIXErrorDomain, code: Int(errno)) }
        let handle = FileHandle(fileDescriptor: fd, closeOnDealloc: true)
        do {
            guard fstat(fd, &entry) == 0 else {
                throw NSError(domain: NSPOSIXErrorDomain, code: Int(errno))
            }
            try validate(entry)
            return handle
        } catch {
            try? handle.close()
            throw error
        }
    }

    private struct UnsafeLog: LocalizedError {
        let path: String
        var errorDescription: String? {
            "Unsafe daemon log at \(path). Expected an owned, single-link regular file with mode 0600. Existing entry was left unchanged."
        }
    }

    private struct UnsafeState: LocalizedError {
        let path: String
        var errorDescription: String? {
            "Unsafe daemon state at \(path). Expected an owned directory with mode 0700. Save agent work and stop the daemon before inspecting ownership, symlinks and ACLs and deliberately migrating old permissions. Existing state was left unchanged."
        }
    }
}
