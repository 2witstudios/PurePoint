import Foundation
import Network

// MARK: - Protocol types matching crates/pu-core/src/protocol.rs

nonisolated enum KillTarget: Encodable {
    case agent(String)
    case worktree(String)
    case all

    func encode(to encoder: Encoder) throws {
        switch self {
        case .agent(let id):
            var container = encoder.container(keyedBy: DynamicCodingKey.self)
            try container.encode(id, forKey: .key("agent"))
        case .worktree(let id):
            var container = encoder.container(keyedBy: DynamicCodingKey.self)
            try container.encode(id, forKey: .key("worktree"))
        case .all:
            var container = encoder.singleValueContainer()
            try container.encode("all")
        }
    }
}

/// Grid command payload matching Rust GridCommand.
nonisolated enum GridCommandPayload: Codable {
    case split(leafId: Int?, axis: String)
    case close(leafId: Int?)
    case focus(leafId: Int?, direction: String?)
    case setAgent(leafId: UInt32, agentId: String)
    case getLayout

    private enum CodingKeys: String, CodingKey {
        case action
        case leafId = "leaf_id"
        case axis
        case direction
        case agentId = "agent_id"
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        let action = try container.decode(String.self, forKey: .action)
        switch action {
        case "split":
            let leafId = try container.decodeIfPresent(Int.self, forKey: .leafId)
            let axis = try container.decodeIfPresent(String.self, forKey: .axis) ?? "v"
            self = .split(leafId: leafId, axis: axis)
        case "close":
            let leafId = try container.decodeIfPresent(Int.self, forKey: .leafId)
            self = .close(leafId: leafId)
        case "focus":
            let leafId = try container.decodeIfPresent(Int.self, forKey: .leafId)
            let direction = try container.decodeIfPresent(String.self, forKey: .direction)
            self = .focus(leafId: leafId, direction: direction)
        case "set_agent":
            let leafId = try container.decode(UInt32.self, forKey: .leafId)
            let agentId = try container.decode(String.self, forKey: .agentId)
            self = .setAgent(leafId: leafId, agentId: agentId)
        default:
            self = .getLayout
        }
    }

    func encode(to encoder: Encoder) throws {
        var container = encoder.container(keyedBy: DynamicCodingKey.self)
        switch self {
        case .split(let leafId, let axis):
            try container.encode("split", forKey: .key("action"))
            if let leafId { try container.encode(leafId, forKey: .key("leaf_id")) }
            try container.encode(axis, forKey: .key("axis"))
        case .close(let leafId):
            try container.encode("close", forKey: .key("action"))
            if let leafId { try container.encode(leafId, forKey: .key("leaf_id")) }
        case .focus(let leafId, let direction):
            try container.encode("focus", forKey: .key("action"))
            if let leafId { try container.encode(leafId, forKey: .key("leaf_id")) }
            if let direction { try container.encode(direction, forKey: .key("direction")) }
        case .setAgent(let leafId, let agentId):
            try container.encode("set_agent", forKey: .key("action"))
            try container.encode(leafId, forKey: .key("leaf_id"))
            try container.encode(agentId, forKey: .key("agent_id"))
        case .getLayout:
            try container.encode("get_layout", forKey: .key("action"))
        }
    }
}

nonisolated enum DaemonRequest: Encodable {
    case health
    case initProject(projectRoot: String)
    case status(projectRoot: String, agentId: String? = nil)
    case attach(agentId: String)
    case input(agentId: String, data: Data)
    case resize(agentId: String, cols: Int, rows: Int)
    case spawn(projectRoot: String, prompt: String, agent: String = "claude",
               name: String? = nil, base: String? = nil, root: Bool = false,
               worktree: String? = nil)
    case kill(projectRoot: String, target: KillTarget)
    case subscribeGrid(projectRoot: String)
    case gridCommand(projectRoot: String, command: GridCommandPayload)
    case shutdown

    func encode(to encoder: Encoder) throws {
        var container = encoder.container(keyedBy: DynamicCodingKey.self)
        switch self {
        case .health:
            try container.encode("health", forKey: .key("type"))
        case .initProject(let projectRoot):
            try container.encode("init", forKey: .key("type"))
            try container.encode(projectRoot, forKey: .key("project_root"))
        case .status(let projectRoot, let agentId):
            try container.encode("status", forKey: .key("type"))
            try container.encode(projectRoot, forKey: .key("project_root"))
            if let agentId { try container.encode(agentId, forKey: .key("agent_id")) }
        case .attach(let agentId):
            try container.encode("attach", forKey: .key("type"))
            try container.encode(agentId, forKey: .key("agent_id"))
        case .input(let agentId, let data):
            try container.encode("input", forKey: .key("type"))
            try container.encode(agentId, forKey: .key("agent_id"))
            try container.encode(data.hexString, forKey: .key("data"))
        case .resize(let agentId, let cols, let rows):
            try container.encode("resize", forKey: .key("type"))
            try container.encode(agentId, forKey: .key("agent_id"))
            try container.encode(cols, forKey: .key("cols"))
            try container.encode(rows, forKey: .key("rows"))
        case .spawn(let projectRoot, let prompt, let agent, let name, let base, let root, let worktree):
            try container.encode("spawn", forKey: .key("type"))
            try container.encode(projectRoot, forKey: .key("project_root"))
            try container.encode(prompt, forKey: .key("prompt"))
            try container.encode(agent, forKey: .key("agent"))
            if let name { try container.encode(name, forKey: .key("name")) }
            if let base { try container.encode(base, forKey: .key("base")) }
            if root { try container.encode(root, forKey: .key("root")) }
            if let worktree { try container.encode(worktree, forKey: .key("worktree")) }
        case .kill(let projectRoot, let target):
            try container.encode("kill", forKey: .key("type"))
            try container.encode(projectRoot, forKey: .key("project_root"))
            try container.encode(target, forKey: .key("target"))
        case .subscribeGrid(let projectRoot):
            try container.encode("subscribe_grid", forKey: .key("type"))
            try container.encode(projectRoot, forKey: .key("project_root"))
        case .gridCommand(let projectRoot, let command):
            try container.encode("grid_command", forKey: .key("type"))
            try container.encode(projectRoot, forKey: .key("project_root"))
            try container.encode(command, forKey: .key("command"))
        case .shutdown:
            try container.encode("shutdown", forKey: .key("type"))
        }
    }
}

nonisolated enum DaemonResponse: Decodable {
    case healthReport(pid: Int, uptimeSeconds: Int, protocolVersion: Int, agentCount: Int)
    case initResult(created: Bool)
    case statusReport(worktrees: [WorktreeEntry], agents: [AgentStatusReport])
    case attachReady(bufferedBytes: Int)
    case output(agentId: String, data: Data)
    case spawnResult(worktreeId: String?, agentId: String, status: String)
    case killResult(killed: [String])
    case gridSubscribed
    case gridLayout(layout: Data)
    case gridEvent(projectRoot: String, command: GridCommandPayload)
    case ok
    case shuttingDown
    case error(code: String, message: String)
    case unknown(type: String)

    private enum CodingKeys: String, CodingKey {
        case type
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        let type = try container.decode(String.self, forKey: .type)

        switch type {
        case "health_report":
            let p = try HealthReportPayload(from: decoder)
            self = .healthReport(pid: p.pid, uptimeSeconds: p.uptimeSeconds,
                                 protocolVersion: p.protocolVersion, agentCount: p.agentCount)
        case "init_result":
            let p = try InitResultPayload(from: decoder)
            self = .initResult(created: p.created)
        case "status_report":
            let p = try StatusReportPayload(from: decoder)
            self = .statusReport(worktrees: p.worktrees, agents: p.agents)
        case "attach_ready":
            let p = try AttachReadyPayload(from: decoder)
            self = .attachReady(bufferedBytes: p.bufferedBytes)
        case "output":
            let p = try OutputPayload(from: decoder)
            self = .output(agentId: p.agentId, data: Data(hexString: p.data))
        case "spawn_result":
            let p = try SpawnResultPayload(from: decoder)
            self = .spawnResult(worktreeId: p.worktreeId, agentId: p.agentId, status: p.status)
        case "kill_result":
            let p = try KillResultPayload(from: decoder)
            self = .killResult(killed: p.killed)
        case "grid_subscribed":
            self = .gridSubscribed
        case "grid_layout":
            let p = try GridLayoutPayload(from: decoder)
            self = .gridLayout(layout: p.layoutData)
        case "grid_event":
            let p = try GridEventPayload(from: decoder)
            self = .gridEvent(projectRoot: p.projectRoot, command: p.command)
        case "ok":
            self = .ok
        case "shutting_down":
            self = .shuttingDown
        case "error":
            let p = try ErrorPayload(from: decoder)
            self = .error(code: p.code, message: p.message)
        default:
            self = .unknown(type: type)
        }
    }
}

struct AgentStatusReport: Decodable {
    let id: String
    let name: String
    let agentType: String
    let status: String
    let pid: Int?
    let exitCode: Int?
    let idleSeconds: Int?
    let worktreeId: String?
    let startedAt: String?

    private enum CodingKeys: String, CodingKey {
        case id, name, status, pid
        case agentType = "agent_type"
        case exitCode = "exit_code"
        case idleSeconds = "idle_seconds"
        case worktreeId = "worktree_id"
        case startedAt = "started_at"
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        id = try container.decode(String.self, forKey: .id)
        name = try container.decode(String.self, forKey: .name)
        agentType = try container.decodeIfPresent(String.self, forKey: .agentType) ?? "unknown"
        status = try container.decode(String.self, forKey: .status)
        pid = try container.decodeIfPresent(Int.self, forKey: .pid)
        exitCode = try container.decodeIfPresent(Int.self, forKey: .exitCode)
        idleSeconds = try container.decodeIfPresent(Int.self, forKey: .idleSeconds)
        worktreeId = try container.decodeIfPresent(String.self, forKey: .worktreeId)
        startedAt = try container.decodeIfPresent(String.self, forKey: .startedAt)
    }
}

// MARK: - Response payload helpers

private struct InitResultPayload: Decodable {
    let created: Bool
}

private struct HealthReportPayload: Decodable {
    let pid: Int
    let uptimeSeconds: Int
    let protocolVersion: Int
    let agentCount: Int

    enum CodingKeys: String, CodingKey {
        case pid
        case uptimeSeconds = "uptime_seconds"
        case protocolVersion = "protocol_version"
        case agentCount = "agent_count"
    }
}

private struct StatusReportPayload: Decodable {
    let worktrees: [WorktreeEntry]
    let agents: [AgentStatusReport]
}

private struct AttachReadyPayload: Decodable {
    let bufferedBytes: Int

    enum CodingKeys: String, CodingKey {
        case bufferedBytes = "buffered_bytes"
    }
}

private struct OutputPayload: Decodable {
    let agentId: String
    let data: String

    enum CodingKeys: String, CodingKey {
        case agentId = "agent_id"
        case data
    }
}

private struct SpawnResultPayload: Decodable {
    let worktreeId: String?
    let agentId: String
    let status: String

    enum CodingKeys: String, CodingKey {
        case worktreeId = "worktree_id"
        case agentId = "agent_id"
        case status
    }
}

private struct KillResultPayload: Decodable {
    let killed: [String]
}

private struct ErrorPayload: Decodable {
    let code: String
    let message: String
}

private struct GridLayoutPayload: Decodable {
    let layout: AnyCodable

    var layoutData: Data {
        (try? JSONEncoder().encode(layout)) ?? Data()
    }

    enum CodingKeys: String, CodingKey { case layout }
}

/// Minimal wrapper so we can round-trip arbitrary JSON.
private struct AnyCodable: Codable {
    let value: Any

    init(from decoder: Decoder) throws {
        let container = try decoder.singleValueContainer()
        if let dict = try? container.decode([String: AnyCodable].self) {
            value = dict.mapValues(\.value)
        } else if let arr = try? container.decode([AnyCodable].self) {
            value = arr.map(\.value)
        } else if let str = try? container.decode(String.self) {
            value = str
        } else if let num = try? container.decode(Double.self) {
            value = num
        } else if let bool = try? container.decode(Bool.self) {
            value = bool
        } else if container.decodeNil() {
            value = NSNull()
        } else {
            value = NSNull()
        }
    }

    func encode(to encoder: Encoder) throws {
        var container = encoder.singleValueContainer()
        switch value {
        case let dict as [String: Any]:
            try container.encode(dict.mapValues { AnyCodable(value: $0) })
        case let arr as [Any]:
            try container.encode(arr.map { AnyCodable(value: $0) })
        case let str as String:
            try container.encode(str)
        case let num as Double:
            try container.encode(num)
        case let bool as Bool:
            try container.encode(bool)
        default:
            try container.encodeNil()
        }
    }

    init(value: Any) { self.value = value }
}

private struct GridEventPayload: Decodable {
    let projectRoot: String
    let command: GridCommandPayload

    enum CodingKeys: String, CodingKey {
        case projectRoot = "project_root"
        case command
    }
}

// MARK: - DaemonClient

nonisolated final class DaemonClient: @unchecked Sendable {
    static let connectionQueue = DispatchQueue(label: "purepoint.daemon.connection")
    private let socketPath: String

    init(socketPath: String? = nil) {
        self.socketPath = socketPath ?? {
            let home = FileManager.default.homeDirectoryForCurrentUser.path
            return "\(home)/.pu/daemon.sock"
        }()
    }

    /// Send a single request and return the response.
    func send(_ request: DaemonRequest) async throws -> DaemonResponse {
        let (connection, reader) = try await connect()
        defer { connection.cancel() }

        try await Self.write(request, to: connection)
        return try await readOne(from: reader)
    }

    /// Connect to the daemon and return the connection + a line reader.
    func connect() async throws -> (NWConnection, DaemonLineReader) {
        let params = NWParameters(tls: nil, tcp: NWProtocolTCP.Options())
        let endpoint = NWEndpoint.unix(path: socketPath)
        let connection = NWConnection(to: endpoint, using: params)

        try await withCheckedThrowingContinuation { (cont: CheckedContinuation<Void, Error>) in
            // nonisolated(unsafe) is safe because the handler runs on the serial connectionQueue
            nonisolated(unsafe) var resumed = false
            connection.stateUpdateHandler = { state in
                guard !resumed else { return }
                switch state {
                case .ready:
                    resumed = true
                    cont.resume()
                case .failed(let err):
                    resumed = true
                    cont.resume(throwing: err)
                case .waiting(let err):
                    // Stale socket: file exists but no listener — fail fast
                    connection.cancel()
                    resumed = true
                    cont.resume(throwing: err)
                case .cancelled:
                    resumed = true
                    cont.resume(throwing: DaemonClientError.cancelled)
                default:
                    break
                }
            }
            connection.start(queue: DaemonClient.connectionQueue)
        }

        let reader = DaemonLineReader(connection: connection)
        return (connection, reader)
    }

    // MARK: - Private

    static func write(_ request: DaemonRequest, to connection: NWConnection) async throws {
        let json = try JSONEncoder().encode(request)
        var message = json
        message.append(0x0A) // newline

        try await withCheckedThrowingContinuation { (cont: CheckedContinuation<Void, Error>) in
            connection.send(content: message, completion: .contentProcessed { error in
                if let error {
                    cont.resume(throwing: error)
                } else {
                    cont.resume()
                }
            })
        }
    }

    private func readOne(from reader: DaemonLineReader) async throws -> DaemonResponse {
        let line = try await reader.readLine()
        return Self.parse(line)
    }

    static func parse(_ data: Data) -> DaemonResponse {
        do {
            return try JSONDecoder().decode(DaemonResponse.self, from: data)
        } catch {
            let preview = String(data: data.prefix(200), encoding: .utf8) ?? "<binary>"
            print("[DaemonClient] parse error: \(error)\n  raw: \(preview)")
            return .unknown(type: "parse_error")
        }
    }
}

// MARK: - Line reader

nonisolated final class DaemonLineReader: @unchecked Sendable {
    private let connection: NWConnection
    private var buffer = Data()
    private var scanOffset = 0

    init(connection: NWConnection) {
        self.connection = connection
    }

    func readLine() async throws -> Data {
        while true {
            if let newlineIndex = buffer[scanOffset...].firstIndex(of: 0x0A) {
                let line = Data(buffer[scanOffset..<newlineIndex])
                scanOffset = newlineIndex + 1
                // Compact when consumed portion exceeds half the buffer
                if scanOffset > buffer.count / 2 {
                    buffer.removeSubrange(..<scanOffset)
                    scanOffset = 0
                }
                return line
            }
            scanOffset = buffer.count
            let chunk = try await readChunk()
            buffer.append(chunk)
        }
    }

    private func readChunk() async throws -> Data {
        try await withCheckedThrowingContinuation { (cont: CheckedContinuation<Data, Error>) in
            connection.receive(minimumIncompleteLength: 1, maximumLength: 65536) { data, _, _, error in
                if let error {
                    cont.resume(throwing: error)
                } else if let data, !data.isEmpty {
                    cont.resume(returning: data)
                } else {
                    cont.resume(throwing: DaemonClientError.eof)
                }
            }
        }
    }
}

// MARK: - Errors

enum DaemonClientError: Error, LocalizedError {
    case eof
    case cancelled
    case notRunning

    var errorDescription: String? {
        switch self {
        case .eof: "Connection to daemon closed"
        case .cancelled: "Connection cancelled"
        case .notRunning: "Daemon is not running"
        }
    }
}

// MARK: - Helpers

private struct DynamicCodingKey: CodingKey {
    var stringValue: String
    var intValue: Int? { nil }

    init?(stringValue: String) { self.stringValue = stringValue }
    init?(intValue: Int) { return nil }

    static func key(_ name: String) -> DynamicCodingKey {
        DynamicCodingKey(stringValue: name)!
    }
}

private let hexDigits: [UInt8] = Array("0123456789abcdef".utf8)

extension Data {
    var hexString: String {
        var chars = [UInt8]()
        chars.reserveCapacity(count * 2)
        for byte in self {
            chars.append(hexDigits[Int(byte >> 4)])
            chars.append(hexDigits[Int(byte & 0x0F)])
        }
        return String(bytes: chars, encoding: .ascii)!
    }

    init(hexString: String) {
        self.init()
        var index = hexString.startIndex
        while index < hexString.endIndex {
            let nextIndex = hexString.index(index, offsetBy: 2, limitedBy: hexString.endIndex) ?? hexString.endIndex
            if let byte = UInt8(hexString[index..<nextIndex], radix: 16) {
                append(byte)
            }
            index = nextIndex
        }
    }
}
