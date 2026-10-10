import Foundation

struct PointGuardRuntime: Sendable {
    struct Manifest: Decodable {
        struct Paths: Decodable {
            let node: String; let pu: String; let entry: String
            let instructions: String; let skills: [String]
        }
        let schemaVersion: Int; let contractVersion: Int; let piVersion: String
        let nodeVersion: String; let architecture: String; let sourceSHA: String; let paths: Paths
    }
    let node: URL; let pu: URL; let entry: URL
    static func load(manifestURL: URL) throws -> Self {
        let manifest = try JSONDecoder().decode(Manifest.self, from: Data(contentsOf: manifestURL))
        guard manifest.schemaVersion == 1, manifest.contractVersion == 1, manifest.piVersion == "1.1.0",
            ["arm64", "x64", "universal"].contains(manifest.architecture)
        else { throw PiChatError("Unsupported Point Guard package. Install a supported PurePoint update.") }
        let resourceRoot = manifestURL.deletingLastPathComponent().standardizedFileURL
        let contents = resourceRoot.deletingLastPathComponent().deletingLastPathComponent().resolvingSymlinksInPath()
        func resolve(_ relative: String, executable: Bool = false) throws -> URL {
            let url = resourceRoot.appendingPathComponent(relative).standardizedFileURL.resolvingSymlinksInPath()
            guard !relative.hasPrefix("/"), url.path.hasPrefix(contents.path + "/"),
                FileManager.default.fileExists(atPath: url.path),
                !executable || FileManager.default.isExecutableFile(atPath: url.path)
            else { throw PiChatError("Point Guard package is incomplete. Reinstall PurePoint; its bundled resources are missing or invalid.") }
            return url
        }
        let node = try resolve(manifest.paths.node, executable: true)
        let pu = try resolve(manifest.paths.pu, executable: true)
        let entry = try resolve(manifest.paths.entry)
        _ = try resolve(manifest.paths.instructions)
        for skill in manifest.paths.skills { _ = try resolve(skill) }
        guard manifest.paths.skills.count >= 2 else { throw PiChatError("Point Guard package is missing its bundled skills.") }
        return Self(node: node, pu: pu, entry: entry)
    }
}
struct PointGuardDescriptor: Decodable, Sendable {
    let schemaVersion: Int; let contractVersion: Int; let pid: Int32; let instanceId: String
    let adminURL: String; let nativeChatURL: String; let chatURL: String?
    let desktopClientId: String; let hostId: String; let certificateSHA256: String
    func belongs(to pid: Int32, instanceId: String) -> Bool {
        self.pid == pid && self.instanceId == instanceId && schemaVersion == 1 && contractVersion == 1
            && Self.localURL(adminURL, scheme: "http", path: "/admin/v1") != nil
            && Self.localURL(nativeChatURL, scheme: "ws", path: "/v1") != nil
            && UUID(uuidString: desktopClientId) != nil
    }
    static func localURL(_ value: String, scheme: String, path: String) -> URL? {
        guard let url = URL(string: value), url.scheme == scheme, url.host == "127.0.0.1",
            url.port.map({ (1...65535).contains($0) }) == true, url.path == path,
            url.user == nil, url.password == nil, url.query == nil, url.fragment == nil else { return nil }
        return url
    }
}
enum PointGuardPrivateFile {
    static func read(_ url: URL, limit: Int = 65536) throws -> Data {
        let values = try url.resourceValues(forKeys: [.isSymbolicLinkKey, .isRegularFileKey])
        let attributes = try FileManager.default.attributesOfItem(atPath: url.path)
        let mode = (attributes[.posixPermissions] as? NSNumber)?.intValue ?? 0
        guard values.isSymbolicLink != true, values.isRegularFile == true, mode & 0o077 == 0,
            (attributes[.size] as? NSNumber)?.intValue ?? Int.max <= limit else {
            throw PiChatError("Point Guard state must be a private regular file. Restore its owner-only permissions before retrying.")
        }
        let data = try Data(contentsOf: url)
        guard data.count <= limit else { throw PiChatError("Point Guard state is too large.") }
        return data
    }
    static func token(_ url: URL) throws -> String {
        let data = try read(url, limit: 4096)
        guard let value = String(data: data, encoding: .utf8)?.trimmingCharacters(in: .whitespacesAndNewlines),
            value.utf8.count >= 32, value.utf8.count <= 1024,
            value.utf8.allSatisfy({ $0 >= 33 && $0 <= 126 }) else { throw PiChatError("Invalid local Point Guard credential.") }
        return value
    }
}

struct PointGuardStartupFailure: Decodable, Sendable {
    let schemaVersion: Int; let pid: Int32; let instanceId: String
    let code: String; let message: String; let recovery: String
    func description(pid: Int32, instanceId: String) -> String? {
        guard schemaVersion == 1, self.pid == pid, self.instanceId == instanceId,
            !message.isEmpty, message.utf8.count <= 4096, recovery.utf8.count <= 4096 else { return nil }
        return message + " " + recovery
    }
}

enum PointGuardWorkingFolder {
    static func validated(_ path: String) throws -> URL {
        let url = URL(fileURLWithPath: path).standardizedFileURL.resolvingSymlinksInPath()
        var directory: ObjCBool = false
        guard path.hasPrefix("/"), FileManager.default.fileExists(atPath: url.path, isDirectory: &directory), directory.boolValue else {
            throw PiChatError("Choose an existing working folder before starting Point Guard.")
        }
        return url
    }
}
