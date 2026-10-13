import Foundation
import Observation

/// One machine in the sidebar: its connection state and the projects its daemon has
/// registered, each a full `ProjectState` whose daemon, files and git are on that machine.
@Observable
@MainActor
final class MachineState: Identifiable {
    let machine: Machine
    nonisolated var id: UUID { machine.id }
    private(set) var state: MachineConnectionState = .disconnected
    private(set) var projects: [ProjectState] = []
    /// The latest failure of an explicit action (start daemon, project discovery).
    var actionError: String?
    private(set) var isStartingDaemon = false

    @ObservationIgnored weak var appState: AppState?
    @ObservationIgnored weak var registry: WorkspaceRegistry?
    @ObservationIgnored private var connection: MachineConnection!
    @ObservationIgnored private var refreshTask: Task<Void, Never>?
    @ObservationIgnored private var hiddenRoots: Set<String>

    /// How often the project list is re-read while the daemon is up.
    private static let refreshInterval: Duration = .seconds(30)

    var endpoint: DaemonEndpoint { .machine(machine.id) }
    var isConnectedOrConnecting: Bool { state != .disconnected }

    init(machine: Machine, appState: AppState?, registry: WorkspaceRegistry?) {
        self.machine = machine
        self.appState = appState
        self.registry = registry
        hiddenRoots = Set(UserDefaults.standard.stringArray(forKey: Self.hiddenKey(machine.id)) ?? [])
        let owner = WeakMachineState()
        connection = MachineConnection(machine: machine) { state in
            Task { @MainActor in owner.value?.apply(state) }
        }
        owner.value = self
    }

    var statusText: String {
        switch state {
        case .disconnected: "Disconnected"
        case .connecting: "Connecting…"
        case .connected(.running): projects.isEmpty ? "Connected" : "\(projects.count) project\(projects.count == 1 ? "" : "s")"
        case .connected(.stopped): "Daemon stopped"
        case .connected(.incompatible(let version)): "Incompatible (protocol \(version))"
        case .offline: "Offline"
        }
    }

    // MARK: - Connection

    func connect() {
        Self.setUserDisconnected(machine.id, false)
        let connection = self.connection!
        Task { await connection.connect() }
    }

    /// Close the SSH forward. The remote daemon and its agents keep running (MACH-004).
    func disconnect(remember: Bool = false) {
        if remember { Self.setUserDisconnected(machine.id, true) }
        connection.forward.terminate()
        let connection = self.connection!
        Task { await connection.disconnect() }
        refreshTask?.cancel()
        refreshTask = nil
        removeAllProjects()
        state = .disconnected
    }

    /// Explicit user action: start the daemon on the machine over ssh (MACH-003).
    func startDaemon() {
        guard !isStartingDaemon else { return }
        isStartingDaemon = true
        actionError = nil
        let connection = self.connection!
        Task {
            let error = await connection.startDaemon()
            isStartingDaemon = false
            if let error { actionError = "Could not start the daemon on \(machine.name): \(error)" }
        }
    }

    private func apply(_ newState: MachineConnectionState) {
        let wasRunning = state.isDaemonRunning
        state = newState
        if newState.isDaemonRunning {
            if !wasRunning { startRefreshing() }
        } else {
            refreshTask?.cancel()
            refreshTask = nil
            // An incompatible daemon gets no further requests; a stopped or unreachable one
            // has nothing to stream. Projects come back with the daemon.
            if case .connected = newState { removeAllProjects() }
            if newState == .disconnected { removeAllProjects() }
        }
    }

    // MARK: - Projects

    private func startRefreshing() {
        refreshTask?.cancel()
        refreshTask = Task { [weak self] in
            while !Task.isCancelled {
                await self?.refreshProjects()
                do { try await Task.sleep(for: Self.refreshInterval) } catch { return }
            }
        }
    }

    /// Discover the daemon's registered projects (`inventory`, kind projects, all roots).
    func refreshProjects() async {
        let client = endpoint.makeClient()
        let response: DaemonResponse
        do {
            response = try await client.send(.inventory(kind: "projects"))
        } catch {
            return
        }
        guard state.isDaemonRunning else { return }
        switch response {
        case .inventory(let report):
            actionError = nil
            setProjects(roots: report.projects.map(\.projectRoot))
        case .error(_, let message):
            actionError = message
        default:
            break
        }
    }

    private func setProjects(roots: [String]) {
        let visible = roots.filter { !hiddenRoots.contains($0) }
        let wanted = Set(visible)
        for project in projects where !wanted.contains(project.projectRoot) {
            forget(project)
        }
        var next: [ProjectState] = []
        for root in visible {
            if let existing = projects.first(where: { $0.projectRoot == root }) {
                next.append(existing)
            } else {
                next.append(makeProject(root: root))
            }
        }
        let added = next.filter { candidate in !projects.contains { $0 === candidate } }
        if next.map(\.projectRoot) != projects.map(\.projectRoot) { projects = next }
        for project in added { project.startWatching() }
    }

    private func makeProject(root: String) -> ProjectState {
        let endpoint = self.endpoint
        let client = endpoint.makeClient()
        let project = ProjectState(
            projectRoot: root, service: DaemonWorkspaceService(client: client), registry: registry,
            endpoint: endpoint, host: DaemonProjectHost(client: client, machineId: machine.id))
        project.appState = appState
        return project
    }

    /// Remove a project from this app only. Nothing on the machine is deleted or stopped.
    func hideProject(root: String) {
        hiddenRoots.insert(root)
        UserDefaults.standard.set(Array(hiddenRoots), forKey: Self.hiddenKey(machine.id))
        if let project = projects.first(where: { $0.projectRoot == root }) {
            forget(project)
            projects.removeAll { $0 === project }
        }
    }

    func removeAllProjects() {
        for project in projects { forget(project) }
        if !projects.isEmpty { projects = [] }
    }

    private func forget(_ project: ProjectState) {
        project.stopWatching()
        registry?.forgetProject(project.projectId)
    }

    // MARK: - Persisted preferences

    private static func hiddenKey(_ id: UUID) -> String {
        "PurePointHiddenRemoteProjects." + id.uuidString.lowercased()
    }

    private static let userDisconnectedKey = "PurePointDisconnectedMachines"

    /// Machines the user disconnected stay disconnected across launches.
    static func isUserDisconnected(_ id: UUID) -> Bool {
        (UserDefaults.standard.stringArray(forKey: userDisconnectedKey) ?? []).contains(id.uuidString.lowercased())
    }

    private static func setUserDisconnected(_ id: UUID, _ value: Bool) {
        var ids = Set(UserDefaults.standard.stringArray(forKey: userDisconnectedKey) ?? [])
        if value { ids.insert(id.uuidString.lowercased()) } else { ids.remove(id.uuidString.lowercased()) }
        UserDefaults.standard.set(Array(ids), forKey: userDisconnectedKey)
    }
}

/// Lets the connection's sendable callback reach its machine without retaining it.
@MainActor
private final class WeakMachineState {
    weak var value: MachineState?
}
