import Foundation

/// Descriptor-relative filesystem boundary against other OS users.
/// Same-account code and root remain trusted; existing state is never repaired.
nonisolated enum DaemonStateDirectory {
    static func prepare(_ directory: URL) throws {
        let fd = try openDirectory(directory, create: true)
        close(fd)
    }

    private static func openDirectory(_ directory: URL, create: Bool) throws -> Int32 {
        var names = directory.path.split(separator: "/").map(String.init)
        guard !names.contains("..") else { throw UnsafeState(path: directory.path) }
        names.removeAll { $0 == "." }
        var fd = open("/", O_RDONLY | O_DIRECTORY | O_CLOEXEC)
        guard fd >= 0 else { throw posixError() }
        do {
            var aliases = 0
            while !names.isEmpty {
                try validateDirectory(fd, privateLeaf: false, path: directory.path)
                let name = names.removeFirst()
                var before = stat()
                if fstatat(fd, name, &before, AT_SYMLINK_NOFOLLOW) != 0 {
                    guard errno == ENOENT && create else { throw posixError() }
                    if mkdirat(fd, name, 0o700) != 0 && errno != EEXIST { throw posixError() }
                    guard fstatat(fd, name, &before, AT_SYMLINK_NOFOLLOW) == 0 else { throw posixError() }
                }
                if before.st_mode & S_IFMT == S_IFLNK {
                    var parent = stat()
                    guard fstat(fd, &parent) == 0 else { throw posixError() }
                    guard !names.isEmpty, before.st_uid == 0, parent.st_uid == 0, aliases < 40 else {
                        throw UnsafeState(path: directory.path)
                    }
                    aliases += 1
                    var target = [CChar](repeating: 0, count: Int(PATH_MAX) + 1)
                    let count = readlinkat(fd, name, &target, Int(PATH_MAX))
                    guard count > 0, count < Int(PATH_MAX) else { throw UnsafeState(path: directory.path) }
                    let path = String(cString: target)
                    let parts = path.split(separator: "/").map(String.init)
                    guard !parts.contains("..") else { throw UnsafeState(path: directory.path) }
                    names = parts.filter { $0 != "." } + names
                    if path.hasPrefix("/") {
                        close(fd)
                        fd = open("/", O_RDONLY | O_DIRECTORY | O_CLOEXEC)
                        guard fd >= 0 else { throw posixError() }
                    }
                    continue
                }
                let child = openat(fd, name, O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC)
                guard child >= 0 else { throw posixError() }
                do {
                    var after = stat()
                    guard fstat(child, &after) == 0 else { throw posixError() }
                    guard before.st_dev == after.st_dev, before.st_ino == after.st_ino else {
                        throw UnsafeState(path: directory.path)
                    }
                    try validateDirectory(child, privateLeaf: names.isEmpty, path: directory.path)
                } catch { close(child); throw error }
                close(fd)
                fd = child
            }
            try validateDirectory(fd, privateLeaf: true, path: directory.path)
            return fd
        } catch { if fd >= 0 { close(fd) }; throw error }
    }

    private static func validateDirectory(_ fd: Int32, privateLeaf: Bool, path: String) throws {
        var entry = stat()
        guard fstat(fd, &entry) == 0 else { throw posixError() }
        guard entry.st_mode & S_IFMT == S_IFDIR else { throw UnsafeState(path: path) }
        if privateLeaf {
            guard entry.st_uid == geteuid(), entry.st_mode & 0o7777 == 0o700 else {
                throw UnsafeState(path: path)
            }
        } else {
            guard entry.st_uid == 0 || entry.st_uid == geteuid(),
                entry.st_mode & 0o022 == 0 || (entry.st_uid == 0 && entry.st_mode & 0o1000 != 0)
            else { throw UnsafeState(path: path) }
        }
        try rejectACLGrants(fd, path: path)
    }

    private static func rejectACLGrants(_ fd: Int32, path: String) throws {
        guard let acl = acl_get_fd_np(fd, ACL_TYPE_EXTENDED) else {
            if errno == ENOENT || errno == ENOTSUP { return }
            throw posixError()
        }
        defer { acl_free(UnsafeMutableRawPointer(acl)) }
        var id = ACL_FIRST_ENTRY
        var entry: acl_entry_t?
        while acl_get_entry(acl, Int32(id.rawValue), &entry) == 0 {
            var tag = ACL_UNDEFINED_TAG
            guard acl_get_tag_type(entry, &tag) == 0 else { throw posixError() }
            guard tag == ACL_EXTENDED_DENY else { throw UnsafeACL(path: path) }
            id = ACL_NEXT_ENTRY
        }
        guard errno == EINVAL else { throw posixError() }
    }

    private static func posixError() -> NSError {
        NSError(domain: NSPOSIXErrorDomain, code: Int(errno))
    }

    static func openLog(_ path: URL) throws -> FileHandle {
        func validate(_ entry: stat) throws {
            guard entry.st_mode & S_IFMT == S_IFREG, entry.st_uid == geteuid(),
                entry.st_nlink == 1, entry.st_mode & 0o7777 == 0o600
            else {
                throw UnsafeLog(path: path.path)
            }
        }
        let directory = try openDirectory(path.deletingLastPathComponent(), create: false)
        defer { close(directory) }
        let name = path.lastPathComponent
        var entry = stat()
        if fstatat(directory, name, &entry, AT_SYMLINK_NOFOLLOW) == 0 {
            try validate(entry)
        } else if errno != ENOENT {
            throw NSError(domain: NSPOSIXErrorDomain, code: Int(errno))
        }
        // No special-file open before validation, and a raced FIFO cannot block.
        let fd = openat(directory, name, O_WRONLY | O_CREAT | O_APPEND | O_CLOEXEC | O_NOFOLLOW | O_NONBLOCK, 0o600)
        guard fd >= 0 else { throw NSError(domain: NSPOSIXErrorDomain, code: Int(errno)) }
        let handle = FileHandle(fileDescriptor: fd, closeOnDealloc: true)
        do {
            guard fstat(fd, &entry) == 0 else {
                throw NSError(domain: NSPOSIXErrorDomain, code: Int(errno))
            }
            try validate(entry)
            try rejectACLGrants(fd, path: path.path)
            return handle
        } catch {
            try? handle.close()
            throw error
        }
    }

    private struct UnsafeACL: LocalizedError {
        let path: String
        var errorDescription: String? {
            "Unsafe grant ACL at \(path). Daemon state, logs and ancestors may have deny-only ACLs; grant ACLs are rejected. Existing entry was left unchanged."
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
            "Unsafe daemon state at \(path). Requires an owned private directory with mode 0700 and ancestors owned by this user or root without foreign write access. Save agent work and stop the daemon before inspecting ownership, symlinks and ACLs and deliberately migrating old permissions. Existing state was left unchanged."
        }
    }
}
