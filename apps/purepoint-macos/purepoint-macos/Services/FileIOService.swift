import Darwin
import Foundation

nonisolated enum FilePreviewError: LocalizedError {
    case tooLarge, unsupported

    var errorDescription: String? {
        switch self {
        case .tooLarge: "File is too large to preview (over 1 MB)."
        case .unsupported: "This file cannot be previewed."
        }
    }
}

/// A file's text as read through a host, with the modification stamp a later save checks.
nonisolated struct HostTextFile: Sendable {
    let content: String
    let isBinary: Bool
    /// Daemon modification time, used as `expected_modified_ms` when saving remotely.
    let modifiedMs: Int64
}

enum FileIOService {
    /// Remote reads without an explicit limit stop at the daemon's default.
    static let remoteReadLimit = 1_048_576

    static func readFile(at path: String, limit: Int? = nil) async throws -> (content: String, isBinary: Bool) {
        try await Task.detached {
            let url = URL(fileURLWithPath: path)
            let data: Data
            if let limit {
                let attributes = try FileManager.default.attributesOfItem(atPath: path)
                guard attributes[.type] as? FileAttributeType == .typeRegular else {
                    throw FilePreviewError.unsupported
                }
                let handle = try FileHandle(forReadingFrom: url)
                defer { try? handle.close() }
                data = try handle.read(upToCount: limit + 1) ?? Data()
                guard data.count <= limit else { throw FilePreviewError.tooLarge }
            } else {
                data = try Data(contentsOf: url)
            }
            return decode(data)
        }.value
    }

    /// Binary detection: a NUL byte in the first 8 KB, or bytes that are not UTF-8.
    nonisolated static func decode(_ data: Data) -> (content: String, isBinary: Bool) {
        let scanLength = min(data.count, 8192)
        let prefix = data.prefix(scanLength)
        if prefix.contains(0x00) {
            return (content: "", isBinary: true)
        }

        guard let content = String(data: data, encoding: .utf8) else {
            return (content: "", isBinary: true)
        }
        return (content: content, isBinary: false)
    }

    /// Read through a project's host. Local hosts keep the behaviour above.
    static func readFile(at path: String, limit: Int? = nil, host: any ProjectHost) async throws -> (
        content: String, isBinary: Bool
    ) {
        if host.isLocal { return try await readFile(at: path, limit: limit) }
        let file = try await readTextFile(at: path, limit: limit, host: host)
        return (file.content, file.isBinary)
    }

    static func readTextFile(at path: String, limit: Int? = nil, host: any ProjectHost) async throws -> HostTextFile {
        if host.isLocal {
            let result = try await readFile(at: path, limit: limit)
            let modified = LocalProjectHost.milliseconds(fileModificationDate(at: path))
            return HostTextFile(content: result.content, isBinary: result.isBinary, modifiedMs: modified)
        }
        let maxBytes = limit ?? remoteReadLimit
        return try await HostIO.run {
            let read = try host.read(path: path, maxBytes: maxBytes)
            // Like local files over the preview limit, an over-long remote file is not shown.
            guard !read.truncated else { throw FilePreviewError.tooLarge }
            let decoded = decode(read.data)
            return HostTextFile(content: decoded.content, isBinary: decoded.isBinary, modifiedMs: read.modifiedMs)
        }
    }

    static func writeFile(content: String, to path: String) async throws {
        try await Task.detached {
            let url = URL(fileURLWithPath: path)
            try content.write(to: url, atomically: true, encoding: .utf8)
        }.value
    }

    /// Save through a host. Remote saves are rejected with a conflict when the file changed
    /// since `expectedModifiedMs`. Returns the new modification stamp.
    @discardableResult
    static func writeFile(content: String, to path: String, host: any ProjectHost, expectedModifiedMs: Int64?)
        async throws -> Int64?
    {
        if host.isLocal {
            try await writeFile(content: content, to: path)
            return LocalProjectHost.milliseconds(fileModificationDate(at: path))
        }
        return try await HostIO.run {
            try host.write(
                path: path, data: Data(content.utf8), expectedModifiedMs: expectedModifiedMs, createDirectories: false
            ).modifiedMs
        }
    }

    static func fileModificationDate(at path: String) -> Date? {
        try? FileManager.default.attributesOfItem(atPath: path)[.modificationDate] as? Date
    }

    static func fileModificationDate(at path: String, host: any ProjectHost) async -> Date? {
        if host.isLocal { return fileModificationDate(at: path) }
        return await HostIO.value { (try? host.stat(path: path))?.modificationDate }
    }

    /// The most the daemon returns from one read (it clamps larger requests).
    nonisolated static let maxLocalCopyBytes = 8 * 1_048_576

    /// Opening a remote file in another app needs a local copy: fetch it to a private temp
    /// file. Copies are removed when the app quits (and leftovers at the next launch).
    static func localCopy(of path: String, host: any ProjectHost) async throws -> URL {
        if host.isLocal { return URL(fileURLWithPath: path) }
        return try await HostIO.run {
            let read = try host.read(path: path, maxBytes: maxLocalCopyBytes)
            guard !read.truncated else { throw LocalCopyError.tooLarge }
            let directory = FileManager.default.temporaryDirectory
                .appendingPathComponent(localCopyPrefix + UUID().uuidString, isDirectory: true)
            try FileManager.default.createDirectory(
                at: directory, withIntermediateDirectories: true, attributes: [.posixPermissions: 0o700])
            let url = directory.appendingPathComponent((path as NSString).lastPathComponent)
            try read.data.write(to: url, options: .atomic)
            quarantine(url)
            return url
        }
    }
}

nonisolated enum LocalCopyError: LocalizedError {
    case tooLarge

    var errorDescription: String? {
        "Remote files over 8 MB cannot be copied to this Mac from PurePoint."
    }
}

extension FileIOService {
    nonisolated static let localCopyPrefix = "purepoint-remote-"

    /// Remove every local copy of a remote file (quit, and leftovers of an earlier run).
    nonisolated static func removeLocalCopies() {
        let temp = FileManager.default.temporaryDirectory
        let names = (try? FileManager.default.contentsOfDirectory(atPath: temp.path)) ?? []
        for name in names where name.hasPrefix(localCopyPrefix) {
            try? FileManager.default.removeItem(at: temp.appendingPathComponent(name))
        }
    }
}

extension FileIOService {
    /// Mark a copy fetched from another machine as downloaded, so Gatekeeper checks it before
    /// anything in it (a `.command`, an app) runs on this Mac.
    nonisolated static func quarantine(_ url: URL) {
        let value = "0083;\(String(Int(Date().timeIntervalSince1970), radix: 16));PurePoint;"
        _ = value.withCString { pointer in
            setxattr(url.path, "com.apple.quarantine", pointer, strlen(pointer), 0, 0)
        }
    }
}
