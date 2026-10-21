import Darwin
import Foundation

/// A computer reachable over SSH that runs its own PurePoint daemon (machines.md).
nonisolated struct Machine: Identifiable, Hashable, Sendable, Codable {
    let id: UUID
    var name: String
    /// An `~/.ssh/config` alias or `user@host[:port]`, passed to ssh as one argument.
    var sshHost: String

    init(id: UUID = UUID(), name: String, sshHost: String) {
        self.id = id
        self.name = name
        self.sshHost = sshHost
    }

    enum CodingKeys: String, CodingKey {
        case id, name
        case sshHost = "ssh_host"
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        let rawId = try container.decode(String.self, forKey: .id)
        guard let id = UUID(uuidString: rawId) else {
            throw DecodingError.dataCorruptedError(forKey: .id, in: container, debugDescription: "Invalid machine id")
        }
        self.id = id
        name = try container.decode(String.self, forKey: .name)
        sshHost = try container.decode(String.self, forKey: .sshHost)
    }

    func encode(to encoder: Encoder) throws {
        var container = encoder.container(keyedBy: CodingKeys.self)
        // Lowercase, as the CLI writes ids — they name `~/.pu/machines/<id>`.
        try container.encode(id.uuidString.lowercased(), forKey: .id)
        try container.encode(name, forKey: .name)
        try container.encode(sshHost, forKey: .sshHost)
    }
}

nonisolated enum MachineRegistryError: LocalizedError, Equatable {
    case unsupportedVersion(Int)
    case invalidName(String)
    case invalidHost(String)
    case duplicateName(String)

    var errorDescription: String? {
        switch self {
        case .unsupportedVersion(let version):
            "Unsupported machine registry version \(version) (expected 1) — update PurePoint."
        case .invalidName:
            "Use 1–64 letters, digits, '.', '_' or '-', starting with a letter or digit."
        case .invalidHost:
            "Use an ~/.ssh/config alias or user@host[:port], without spaces."
        case .duplicateName(let name):
            "A machine named \"\(name)\" already exists."
        }
    }
}

/// `~/.pu/machines.json`, shared with the CLI (`crates/pu-core/src/machines.rs`):
/// `{ "version": 1, "machines": [ { "id", "name", "ssh_host" } ] }`.
nonisolated enum MachineRegistryStore {
    static let version = 1

    private struct Document: Codable {
        let version: Int
        var machines: [Machine]
    }

    static func decode(_ data: Data) throws -> [Machine] {
        let document = try JSONDecoder().decode(Document.self, from: data)
        guard document.version == version else { throw MachineRegistryError.unsupportedVersion(document.version) }
        return document.machines
    }

    static func encode(_ machines: [Machine]) throws -> Data {
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.prettyPrinted, .sortedKeys, .withoutEscapingSlashes]
        var data = try encoder.encode(Document(version: version, machines: machines))
        data.append(0x0A)
        return data
    }

    /// A missing file is an empty registry.
    static func load(path: String = MachinePaths.registryPath) throws -> [Machine] {
        guard let data = FileManager.default.contents(atPath: path) else { return [] }
        return try decode(data)
    }

    /// Load, change and save under the same exclusive `machines.json.lock` the CLI takes,
    /// so concurrent edits from either never lose each other.
    @discardableResult
    static func update<T>(path: String = MachinePaths.registryPath, _ change: (inout [Machine]) throws -> T) throws -> T {
        let directory = (path as NSString).deletingLastPathComponent
        try FileManager.default.createDirectory(atPath: directory, withIntermediateDirectories: true)
        let lockFd = open(path + ".lock", O_CREAT | O_WRONLY | O_CLOEXEC, 0o600)
        guard lockFd >= 0 else { throw CocoaError(.fileWriteNoPermission) }
        defer { close(lockFd) }
        guard flock(lockFd, LOCK_EX) == 0 else { throw CocoaError(.fileLocking) }
        defer { flock(lockFd, LOCK_UN) }

        var machines = try load(path: path)
        let result = try change(&machines)
        let data = try encode(machines)
        let temp = directory + "/.machines.json.tmp.\(UUID().uuidString.lowercased())"
        guard FileManager.default.createFile(atPath: temp, contents: data, attributes: [.posixPermissions: 0o600]) else {
            throw CocoaError(.fileWriteUnknown)
        }
        guard rename(temp, path) == 0 else {
            try? FileManager.default.removeItem(atPath: temp)
            throw CocoaError(.fileWriteUnknown)
        }
        return result
    }

    static func add(name: String, sshHost: String, to machines: inout [Machine]) throws -> Machine {
        try validate(name: name)
        try validate(sshHost: sshHost)
        if let existing = machines.first(where: { $0.name.caseInsensitiveCompare(name) == .orderedSame }) {
            throw MachineRegistryError.duplicateName(existing.name)
        }
        let machine = Machine(name: name, sshHost: sshHost)
        machines.append(machine)
        return machine
    }

    /// Names are what `pu --machine` matches: `[A-Za-z0-9._-]`, starting alphanumeric.
    static func validate(name: String) throws {
        let allowed = CharacterSet(charactersIn: "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789._-")
        guard let first = name.unicodeScalars.first, name.count <= 64,
            CharacterSet.alphanumerics.contains(first), first.isASCII,
            name.unicodeScalars.allSatisfy({ allowed.contains($0) })
        else { throw MachineRegistryError.invalidName(name) }
    }

    /// Passed to ssh as one argument, so it must not look like an option.
    static func validate(sshHost: String) throws {
        guard !sshHost.isEmpty, sshHost.utf8.count <= 255, !sshHost.hasPrefix("-"),
            !sshHost.unicodeScalars.contains(where: {
                CharacterSet.whitespacesAndNewlines.contains($0) || CharacterSet.controlCharacters.contains($0)
            })
        else { throw MachineRegistryError.invalidHost(sshHost) }
    }
}

/// `Host` aliases from `~/.ssh/config`, offered when adding a machine.
nonisolated enum SSHConfig {
    #if os(macOS)
    static var defaultPath: String { FileManager.default.homeDirectoryForCurrentUser.path + "/.ssh/config" }

    static func hostAliases(path: String = defaultPath) -> [String] {
        guard let text = try? String(contentsOfFile: path, encoding: .utf8) else { return [] }
        return hostAliases(from: text)
    }
    #endif

    /// Every concrete alias on `Host` lines; patterns (`*`, `?`, negations) are skipped.
    static func hostAliases(from text: String) -> [String] {
        var aliases: [String] = []
        for rawLine in text.components(separatedBy: .newlines) {
            let line = rawLine.trimmingCharacters(in: .whitespaces)
            guard !line.isEmpty, !line.hasPrefix("#") else { continue }
            // `Host a b`, `Host=a`, `host  a` are all valid ssh_config syntax.
            guard let separator = line.firstIndex(where: { $0 == " " || $0 == "\t" || $0 == "=" }) else { continue }
            guard line[..<separator].caseInsensitiveCompare("host") == .orderedSame else { continue }
            let values = line[line.index(after: separator)...]
                .trimmingCharacters(in: CharacterSet(charactersIn: " \t="))
                .split(whereSeparator: { $0 == " " || $0 == "\t" })
            for value in values {
                let alias = String(value).trimmingCharacters(in: CharacterSet(charactersIn: "\""))
                guard !alias.isEmpty, !alias.contains(where: { "*?!".contains($0) }), !aliases.contains(alias) else {
                    continue
                }
                aliases.append(alias)
            }
        }
        return aliases
    }
}
