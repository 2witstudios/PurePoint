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
    init(socketPath: String? = nil) {}
    func send(_ request: DaemonRequest) async throws -> DaemonResponse { .ok }
}
// Project hosts: only the local, file-free surface AppState and persistence name.
struct HostFileData { let data: Data; let modifiedMs: Int64; let truncated: Bool }
struct HostFileStat { let modifiedMs: Int64 }
enum HostError: Error { case notFound(String), conflict(String) }
protocol ProjectHost: Sendable {
    var isLocal: Bool { get }
    func read(path: String, maxBytes: Int) throws -> HostFileData
    func write(path: String, data: Data, expectedModifiedMs: Int64?, createDirectories: Bool) throws -> HostFileStat
}
struct LocalProjectHost: ProjectHost {
    static let shared = LocalProjectHost()
    var isLocal: Bool { true }
    func read(path: String, maxBytes: Int) throws -> HostFileData { throw CocoaError(.fileNoSuchFile) }
    func write(path: String, data: Data, expectedModifiedMs: Int64?, createDirectories: Bool) throws -> HostFileStat {
        throw CocoaError(.fileWriteUnknown)
    }
}
struct UnavailableProjectHost: ProjectHost {
    var isLocal: Bool { false }
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
    func disconnect(remember: Bool = false) {}
    func disconnectForTermination(deadline: DispatchTime) {}
    func update(from machine: Machine) {}
    func hideProject(root: String) {}
    func removeProject(root: String, suspendAgents: Bool) async throws {}
    static func isUserDisconnected(_ id: UUID) -> Bool { true }
}
enum DaemonLifecycle {
    static func findBinary() -> String? { nil }
    static func didLaunchDaemon() async -> Bool { false }
    static func restartDaemon() async throws {}
    static func ensureDaemon() async throws {}
}
struct MachineOpenProjectError: LocalizedError {
    let message: String
}
actor DaemonProjectsSubscription {
    init(client: DaemonClient) {}
    func start(onChange: @escaping @MainActor ([String]) -> Void) async {}
    func stop() {}
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
    var key: ProjectKey { ProjectKey(id: projectRoot) }
    var isRemote: Bool { false }
    let host: any ProjectHost = LocalProjectHost.shared
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
