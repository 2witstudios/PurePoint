import Foundation
import SwiftUI

// Only external services/lifecycle are stubbed. AppState, registry, persistence,
// workspace models, protocol serialization and surface spawn routing are production code.
@MainActor final class PiChatModel {}
@MainActor final class PointGuardServiceModel {
    func stopForTermination() {}
    func start(chat: PiChatModel) {}
}
@MainActor final class AgentsHubState {}
@MainActor final class AgentSignalNotifications {
    static let shared = AgentSignalNotifications()
    var open: ((AgentSignalAlertPolicy.Agent) async -> Bool)?
    func update(_ agents: [AgentSignalAlertPolicy.Agent], foregroundIdentity: String?) {}
    func refreshBadge() {}
}
@MainActor final class AgentConfigState {}
@MainActor final class ScheduleState {}
@MainActor final class TriggersState {}
enum SidebarSelection: Equatable {
    case workspace(String), worktree(String), project(String), channel(String), nav(Navigation)
    enum Navigation { case dashboard }
}
struct DaemonWorkspaceService: WorkspaceService {
    func loadWorkspace(projectRoot: String) async throws -> WorkspaceSnapshot {
        WorkspaceSnapshot(worktrees: [], rootAgents: [])
    }
    func manifestPath(projectRoot: String) -> String { "/dev/null" }
}
struct DaemonClient {
    init(socketPath: String? = nil, transport: (any DaemonTransport)? = nil) {}
    func send(_ request: DaemonRequest) async throws -> DaemonResponse { .ok }
}
protocol DaemonTransport: Sendable {}
struct UnixSocketTransport: DaemonTransport { let socketPath: String }
// Project hosts: only the local surface AppState and persistence name. The local host
// reads and writes the real disk, as layout saves through it must.
struct HostFileData { let data: Data; let modifiedMs: Int64; let truncated: Bool }
struct HostFileStat {
    let exists: Bool
    let modifiedMs: Int64
    static let missing = HostFileStat(exists: false, modifiedMs: 0)
}
enum HostError: Error { case notFound(String), conflict(String) }
protocol ProjectHost: Sendable {
    var isLocal: Bool { get }
    func stat(path: String) throws -> HostFileStat
    func read(path: String, maxBytes: Int) throws -> HostFileData
    func write(path: String, data: Data, expectedModifiedMs: Int64?, createDirectories: Bool) throws -> HostFileStat
}
struct LocalProjectHost: ProjectHost {
    static let shared = LocalProjectHost()
    var isLocal: Bool { true }
    func stat(path: String) throws -> HostFileStat {
        guard let attributes = try? FileManager.default.attributesOfItem(atPath: path) else { return .missing }
        return HostFileStat(exists: true, modifiedMs: Self.milliseconds(attributes[.modificationDate] as? Date))
    }
    func read(path: String, maxBytes: Int) throws -> HostFileData {
        let data = try Data(contentsOf: URL(fileURLWithPath: path))
        return HostFileData(
            data: data.prefix(maxBytes), modifiedMs: try stat(path: path).modifiedMs, truncated: data.count > maxBytes)
    }
    func write(path: String, data: Data, expectedModifiedMs: Int64?, createDirectories: Bool) throws -> HostFileStat {
        if let expectedModifiedMs, try stat(path: path).modifiedMs != expectedModifiedMs {
            throw HostError.conflict(path)
        }
        let url = URL(fileURLWithPath: path)
        if createDirectories {
            try FileManager.default.createDirectory(
                at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
        }
        try data.write(to: url, options: .atomic)
        return try stat(path: path)
    }
    static func milliseconds(_ date: Date?) -> Int64 {
        Int64(((date?.timeIntervalSince1970 ?? 0) * 1000).rounded(.down))
    }
}
enum HostIO {
    static func value<T: Sendable>(_ work: @escaping @Sendable () -> T) async -> T { work() }
}
struct UnavailableProjectHost: ProjectHost {
    var isLocal: Bool { false }
    func stat(path: String) throws -> HostFileStat { .missing }
    func read(path: String, maxBytes: Int) throws -> HostFileData { throw HostError.notFound(path) }
    func write(path: String, data: Data, expectedModifiedMs: Int64?, createDirectories: Bool) throws -> HostFileStat {
        throw HostError.notFound(path)
    }
}
enum FileIOService {
    static func removeLocalCopies() {}
}
@MainActor final class MachineState {
    let machine: Machine
    var projects: [ProjectState] = []
    init(machine: Machine, appState: AppState?, registry: WorkspaceRegistry?) { self.machine = machine }
    func connect() {}
    func disconnect(remember: Bool = false, keepUnsaved: Bool = false) {}
    func disconnectForTermination(deadline: DispatchTime) {}
    func update(from machine: Machine) {}
    func hideProject(root: String) {}
    static func isUserDisconnected(_ id: UUID) -> Bool { true }
}
enum DaemonLifecycle {
    static func findBinary() -> String? { nil }
    static func didLaunchDaemon() async -> Bool { false }
    static func restartDaemon() async throws {}
}
@MainActor final class ManifestWatcher {
    init(path: String, onChange: @escaping () -> Void) {}
    func stop() {}
}
@MainActor final class MachineRegistryWatcher {
    init(path: String = MachinePaths.registryPath, onChange: @escaping @MainActor () -> Void) {}
    func retry() {}
    var isWatching: Bool { false }
    func stop() {}
}
@MainActor final class ProjectState {
    let projectRoot: String
    var projectId: String { projectRoot }
    var isRemote: Bool { false }
    var isUnavailable = false
    let host: any ProjectHost = LocalProjectHost.shared
    let remoteLayoutSaver: RemoteLayoutSaver? = nil
    let client = DaemonClient()
    var rootAgents: [AgentModel] = []
    var worktrees: [WorktreeModel] = []
    weak var appState: AppState?
    weak var registry: WorkspaceRegistry?
    var requests: [DaemonRequest] = []
    init(projectRoot: String, service: any WorkspaceService, registry: WorkspaceRegistry?) {
        self.projectRoot = projectRoot
        self.registry = registry
    }
    func startWatching() {}
    func stopWatching() {}
    func agent(byId id: String) -> AgentModel? { allAgents.first { $0.id == id } }
    var allAgents: [AgentModel] { rootAgents + worktrees.flatMap(\.agents) }
    func worktreeId(forAgentId agentId: String) -> String? {
        worktrees.first { $0.agents.contains { $0.id == agentId } }?.id
    }
    func refresh() {}
    func sendDaemonRequest(
        _ request: DaemonRequest, onFailure: (() -> Void)? = nil,
        onSuccess: @escaping (DaemonResponse) -> Void
    ) {
        requests.append(request)
        onSuccess(.spawnResult(worktreeId: nil, agentId: "ag-spawned", status: "running"))
    }
}

// Rendering dependencies are outside routing verification. Keep the production
// PointGuardView in the package so its environment and folder routing typecheck.
struct PaneGridView: View {
    let workspaceId: String
    var body: some View { EmptyView() }
}
struct PiPointGuardChatView: View {
    let model: PiChatModel
    @Binding var showSidebar: Bool
    let isFocused: Bool
    var tabBar: AnyView = AnyView(EmptyView())
    var body: some View { EmptyView() }
}
extension Notification.Name {
    static let toggleChatSidebar = Notification.Name("PP_toggleChatSidebar")
}

struct PointGuardPaneControls: View {
    let model: PiChatModel
    var body: some View { EmptyView() }
}
