import Foundation

/// Project identity: `(machineId, projectRoot)` (MACH-005). `machineId == nil` is the local machine.
///
/// The string form (`id`) is what the registry, sidebar, selection and persisted UI state key
/// projects by. A local key is the bare root — exactly the string those places used before
/// machines existed, so saved layouts, selections and channel drafts keep working. A remote key
/// is `machine:<uuid>:<root>`, which can never collide with a local root (roots are absolute).
nonisolated struct ProjectKey: Hashable, Sendable {
    let machineId: UUID?
    /// Absolute path of the project root on the machine that owns it.
    let root: String

    init(machineId: UUID? = nil, root: String) {
        self.machineId = machineId
        self.root = root
    }

    static let remotePrefix = "machine:"

    var isRemote: Bool { machineId != nil }

    var id: String {
        guard let machineId else { return root }
        return Self.remotePrefix + machineId.uuidString.lowercased() + ":" + root
    }

    /// Parse a key's string form. Anything that is not a well-formed remote key is a local root.
    init(id: String) {
        if id.hasPrefix(Self.remotePrefix) {
            let rest = id.dropFirst(Self.remotePrefix.count)
            if let colon = rest.firstIndex(of: ":"),
                let uuid = UUID(uuidString: String(rest[rest.startIndex..<colon]))
            {
                self.init(machineId: uuid, root: String(rest[rest.index(after: colon)...]))
                return
            }
        }
        self.init(machineId: nil, root: id)
    }

    var projectName: String { URL(fileURLWithPath: root).lastPathComponent }
}

/// Where a project's daemon listens. Local is `~/.pu/daemon.sock`; a machine's endpoint is the
/// local end of its SSH forward, `~/.pu/machines/<id>/daemon.sock`.
nonisolated struct DaemonEndpoint: Hashable, Sendable {
    let machineId: UUID?
    let socketPath: String

    var isLocal: Bool { machineId == nil }

    static var local: DaemonEndpoint {
        DaemonEndpoint(machineId: nil, socketPath: localSocketPath)
    }

    static var localSocketPath: String {
        #if os(macOS)
        FileManager.default.homeDirectoryForCurrentUser.path + "/.pu/daemon.sock"
        #else
        // iOS has no local daemon; this path inside the app container never answers.
        NSHomeDirectory() + "/.pu/daemon.sock"
        #endif
    }

    static func machine(_ id: UUID) -> DaemonEndpoint {
        DaemonEndpoint(machineId: id, socketPath: MachinePaths.socketPath(machineId: id))
    }

    /// How this endpoint is reached. With no provider installed (macOS) it is the Unix socket
    /// at `socketPath`; a platform that reaches machines another way (the iPad, over SSH)
    /// installs `transportProvider` and every client made from an endpoint uses it.
    var transport: any DaemonTransport {
        Self.transportProvider?(self) ?? UnixSocketTransport(socketPath: socketPath)
    }

    func makeClient() -> DaemonClient { DaemonClient(socketPath: socketPath, transport: transport) }

    typealias TransportProvider = @Sendable (DaemonEndpoint) -> any DaemonTransport

    /// The platform's transport hook. Nil on macOS.
    static var transportProvider: TransportProvider? {
        get { providerLock.withLock { installedProvider } }
        set { providerLock.withLock { installedProvider = newValue } }
    }

    private static let providerLock = NSLock()
    nonisolated(unsafe) private static var installedProvider: TransportProvider?
}

/// Filesystem locations shared with the CLI (`crates/pu-core/src/paths.rs`).
nonisolated enum MachinePaths {
    #if os(macOS)
    static var puDirectory: String { FileManager.default.homeDirectoryForCurrentUser.path + "/.pu" }
    #else
    static var puDirectory: String { NSHomeDirectory() + "/.pu" }
    #endif
    static var registryPath: String { puDirectory + "/machines.json" }
    static var machinesDirectory: String { puDirectory + "/machines" }

    /// Machine ids are written lowercase, as the CLI's UUID formatter does, so both name the
    /// same directory.
    static func directory(machineId: UUID) -> String {
        machinesDirectory + "/" + machineId.uuidString.lowercased()
    }

    static func socketPath(machineId: UUID) -> String {
        directory(machineId: machineId) + "/daemon.sock"
    }

    /// Create `~/.pu/machines/<id>` with both levels at mode 0700.
    static func ensureDirectory(machineId: UUID) throws -> String {
        let fm = FileManager.default
        for dir in [machinesDirectory, directory(machineId: machineId)] {
            try fm.createDirectory(
                atPath: dir, withIntermediateDirectories: true, attributes: [.posixPermissions: 0o700])
            try fm.setAttributes([.posixPermissions: 0o700], ofItemAtPath: dir)
        }
        return directory(machineId: machineId)
    }
}
