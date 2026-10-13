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
    /// Bumped when the hidden set changes so the sidebar menu re-reads the count.
    private(set) var hiddenCountRevision = 0

    @ObservationIgnored weak var appState: AppState?
    @ObservationIgnored weak var registry: WorkspaceRegistry?
    @ObservationIgnored private var connection: MachineConnection!
    @ObservationIgnored private var refreshTask: Task<Void, Never>?
    @ObservationIgnored private var hiddenRoots: Set<String>
    /// Closing the forward after pending layout saves were flushed; a reconnect waits for it.
    @ObservationIgnored private var disconnectTask: Task<Void, Never>?
    @ObservationIgnored private var isUserDisconnecting = false

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
        isUserDisconnecting = false
        let connection = self.connection!
        let pending = disconnectTask
        Task {
            await pending?.value
            await connection.connect()
        }
    }

    /// Close the SSH forward. The remote daemon and its agents keep running (MACH-004).
    /// Layout changes still pending are written through the forward first (bounded), so
    /// closing it cannot cut off the last save. Callers confirm discarding unsaved file tabs
    /// (`unsavedFileNames`) before an explicit disconnect.
    func disconnect(remember: Bool = false) {
        if remember { Self.setUserDisconnected(machine.id, true) }
        isUserDisconnecting = true
        refreshTask?.cancel()
        refreshTask = nil
        let savers = removeAllProjects(keepingUnsaved: false)
        state = .disconnected
        let connection = self.connection!
        let previous = disconnectTask
        disconnectTask = Task {
            await previous?.value
            // No more probes or reports, but the forward stays up for the flush.
            await connection.stopSupervising()
            await Self.flush(savers, timeout: .seconds(2))
            connection.forward.terminate()
            await connection.disconnect()
        }
    }

    /// Quit: write pending layout saves synchronously (bounded), then close the forward.
    func disconnectForTermination(deadline: DispatchTime) {
        isUserDisconnecting = true
        refreshTask?.cancel()
        refreshTask = nil
        for saver in removeAllProjects(keepingUnsaved: false) { saver.flush(until: deadline) }
        state = .disconnected
        let connection = self.connection!
        connection.terminateForQuit()
        Task { await connection.disconnect() }
    }

    /// Names of files with unsaved edits in this machine's projects — file-pane tabs and the
    /// editors of worktree and checkout views — which disconnecting would drop.
    var unsavedFileNames: [String] {
        var names: [String] = []
        for project in projects {
            for name in unsavedFileNames(in: project) where !names.contains(name) { names.append(name) }
        }
        return names
    }

    func unsavedFileNames(in project: ProjectState) -> [String] {
        let tabs = registry?.unsavedFileNames(inProject: project.projectId) ?? []
        let editors = OpenEditors.unsavedFileNames(hostIdentity: project.host.identity, root: project.projectRoot)
        return tabs + editors.filter { !tabs.contains($0) }
    }

    /// Projects hidden from the sidebar; their daemon still has them registered.
    var hiddenProjectCount: Int { hiddenRoots.count }

    /// Bring back every hidden project (they reappear with the next project refresh).
    func showHiddenProjects() {
        hiddenRoots.removeAll()
        UserDefaults.standard.removeObject(forKey: Self.hiddenKey(machine.id))
        hiddenCountRevision += 1
        guard state.isDaemonRunning else { return }
        Task { await refreshProjects() }
    }

    private static func flush(_ savers: [RemoteLayoutSaver], timeout: Duration) async {
        guard !savers.isEmpty else { return }
        let seconds = Double(timeout.components.seconds) + Double(timeout.components.attoseconds) / 1e18
        await HostIO.value {
            let deadline = DispatchTime.now() + seconds
            for saver in savers { saver.flush(until: deadline) }
        }
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
        // Reports still in flight from a connection the user closed must not reopen it.
        if isUserDisconnecting, newState != .disconnected { return }
        let wasRunning = state.isDaemonRunning
        state = newState
        if newState.isDaemonRunning {
            if !wasRunning { startRefreshing() }
        } else {
            refreshTask?.cancel()
            refreshTask = nil
            // An incompatible daemon gets no further requests; a stopped one (after several
            // failed probes) has nothing to stream. Projects come back with the daemon. While
            // only the SSH link is down (connecting/offline) they stay, as do projects with
            // unsaved file tabs, which are shown as unavailable instead.
            if case .connected = newState { removeAllProjects(keepingUnsaved: true) }
            if newState == .disconnected { removeAllProjects(keepingUnsaved: true) }
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
            if state.isDaemonRunning { actionError = "Could not list projects on \(machine.name): \(error.localizedDescription)" }
            return
        }
        guard state.isDaemonRunning else { return }
        switch response {
        case .inventory(let report):
            actionError = nil
            setProjects(roots: report.projects.map(\.projectRoot))
        case .error(_, let message):
            actionError = "Could not list projects on \(machine.name): \(message)"
        default:
            break
        }
    }

    private func setProjects(roots: [String]) {
        let visible = roots.filter { !hiddenRoots.contains($0) }
        let wanted = Set(visible)
        var kept: [ProjectState] = []
        for project in projects where !wanted.contains(project.projectRoot) {
            if hasUnsavedTabs(project) {
                // Gone from the daemon, but its unsaved edits are still open here.
                markUnavailable(project)
                kept.append(project)
            } else {
                forget(project)
            }
        }
        var next: [ProjectState] = []
        for root in visible {
            if let existing = projects.first(where: { $0.projectRoot == root }) {
                next.append(existing)
            } else {
                next.append(makeProject(root: root))
            }
        }
        next += kept
        let added = next.filter { candidate in !projects.contains { $0 === candidate } }
        let restored = next.filter { $0.isUnavailable && wanted.contains($0.projectRoot) }
        if next.map(\.projectRoot) != projects.map(\.projectRoot) { projects = next }
        for project in added { project.startWatching() }
        for project in restored {
            project.isUnavailable = false
            project.startWatching()
        }
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
    /// Pending layout saves are written, not dropped. Callers confirm discarding unsaved file
    /// tabs first.
    func hideProject(root: String) {
        hiddenRoots.insert(root)
        hiddenCountRevision += 1
        UserDefaults.standard.set(Array(hiddenRoots), forKey: Self.hiddenKey(machine.id))
        if let project = projects.first(where: { $0.projectRoot == root }) {
            forget(project)
            projects.removeAll { $0 === project }
        }
    }

    /// Drop every project, returning the savers whose queued layout writes should be flushed
    /// before the forward closes. With `keepingUnsaved`, a project holding unsaved file tabs
    /// stays (unwatched, shown as unavailable) so its edits are not lost.
    @discardableResult
    func removeAllProjects(keepingUnsaved: Bool) -> [RemoteLayoutSaver] {
        var savers: [RemoteLayoutSaver] = []
        var kept: [ProjectState] = []
        for project in projects {
            if keepingUnsaved, hasUnsavedTabs(project) {
                markUnavailable(project)
                kept.append(project)
            } else if let saver = forget(project) {
                savers.append(saver)
            }
        }
        if projects.map(\.projectRoot) != kept.map(\.projectRoot) { projects = kept }
        return savers
    }

    private func hasUnsavedTabs(_ project: ProjectState) -> Bool {
        !unsavedFileNames(in: project).isEmpty
    }

    private func markUnavailable(_ project: ProjectState) {
        guard !project.isUnavailable else { return }
        project.stopWatching()
        project.isUnavailable = true
    }

    @discardableResult
    private func forget(_ project: ProjectState) -> RemoteLayoutSaver? {
        project.stopWatching()
        registry?.forgetProject(project.projectId)
        return project.remoteLayoutSaver
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
