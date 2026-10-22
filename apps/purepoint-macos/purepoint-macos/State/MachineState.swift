import Foundation
import Observation

/// One machine in the sidebar: its connection state and the projects its daemon has
/// registered, each a full `ProjectState` whose daemon, files and git are on that machine.
@Observable
@MainActor
final class MachineState: Identifiable {
    var machine: Machine
    private let machineId: UUID
    nonisolated var id: UUID { machineId }
    private(set) var state: MachineConnectionState = .disconnected
    private(set) var projects: [ProjectState] = []
    /// The latest failure of an explicit action (start daemon, project discovery).
    var actionError: String?
    private(set) var isStartingDaemon = false
    /// Bumped when the hidden set changes so the sidebar menu re-reads the count.
    private(set) var hiddenCountRevision = 0
    /// Whether the machine has reported its project list at least once. `projects` is empty
    /// both before the first inventory reply and for a machine with nothing registered, so
    /// anything that has to tell the two apart — the sidebar replaying which rows are closed
    /// — reads this instead.
    private(set) var hasLoadedProjects = false
    #if os(iOS)
    /// The host key the last connect refused: one to confirm on first use, or one that
    /// differs from the pin (IPAD-009). Nil once a connect passes the host-key check.
    private(set) var hostKeyRefusal: SSHTrustError?
    #endif

    /// Whether this machine answers for the rows it shows: its daemon is running and has
    /// reported its list at least once. A machine that is connecting, offline, or whose
    /// daemon is stopped shows rows that are absent because it cannot answer, not because
    /// they are gone — so the sidebar must not treat that as the whole tree until the
    /// forward is back and an inventory has answered.
    var reportsCompleteProjects: Bool {
        state.isDaemonRunning && hasLoadedProjects
    }

    @ObservationIgnored weak var appState: AppState?
    @ObservationIgnored weak var registry: WorkspaceRegistry?
    @ObservationIgnored private var connection: MachineConnection!
    @ObservationIgnored private var connectionGeneration = 0
    @ObservationIgnored private var refreshTask: Task<Void, Never>?
    /// Pushes the daemon's project list on every add or remove; the poll is the fallback.
    @ObservationIgnored private var projectsSubscription: DaemonProjectsSubscription?
    @ObservationIgnored private var hiddenRoots: Set<String>
    /// Closing the forward after pending layout saves were flushed; a reconnect waits for it.
    @ObservationIgnored private var disconnectTask: Task<Void, Never>?
    @ObservationIgnored private var isUserDisconnecting = false

    /// How often the project list is re-read while the daemon is up, in case a pushed
    /// change was missed.
    private static let refreshInterval: Duration = .seconds(30)
    /// Bumped by each local move, so a project list fetched before it can't undo it.
    @ObservationIgnored private var orderRevision = 0
    @ObservationIgnored private lazy var orderSaver = ProjectOrderSaver(endpoint: endpoint)

    var endpoint: DaemonEndpoint { .machine(machine.id) }
    var isConnectedOrConnecting: Bool { state != .disconnected }

    init(machine: Machine, appState: AppState?, registry: WorkspaceRegistry?) {
        self.machine = machine
        self.machineId = machine.id
        self.appState = appState
        self.registry = registry
        hiddenRoots = Set(UserDefaults.standard.stringArray(forKey: Self.hiddenKey(machine.id)) ?? [])
        connection = Self.makeConnection(for: self, generation: connectionGeneration)
    }

    /// Update from a fresh registry read: refresh the name, and when the SSH host changed,
    /// recreate the connection on the new host. Called by `AppState.reloadMachines()`.
    func update(from machine: Machine) {
        guard machine.id == self.machine.id else { return }
        let hostChanged = machine.sshHost != self.machine.sshHost
        self.machine = machine
        guard hostChanged else { return }
        let wasActive = state != .disconnected
        // Preserve unsaved projects across the host change — the user didn't initiate this.
        disconnect(keepUnsaved: true)
        connectionGeneration += 1
        connection = Self.makeConnection(for: self, generation: connectionGeneration)
        if wasActive && !Self.isUserDisconnected(machine.id) { connect() }
    }

    @MainActor
    private static func makeConnection(for state: MachineState, generation: Int) -> MachineConnection {
        let owner = WeakMachineState()
        #if os(iOS)
        let connection = MachineConnection(
            machine: state.machine,
            onTrust: { refusal in
                Task { @MainActor in
                    guard owner.value?.connectionGeneration == generation else { return }
                    owner.value?.applyHostKeyRefusal(refusal)
                }
            },
            onState: { newState in
                Task { @MainActor in
                    guard owner.value?.connectionGeneration == generation else { return }
                    owner.value?.apply(newState)
                }
            })
        #else
        let connection = MachineConnection(machine: state.machine) { newState in
            Task { @MainActor in
                // Ignore callbacks from a superseded connection.
                guard owner.value?.connectionGeneration == generation else { return }
                owner.value?.apply(newState)
            }
        }
        #endif
        owner.value = state
        return connection
    }

    var statusText: String {
        #if os(iOS)
        switch hostKeyRefusal {
        case .hostKeyChanged?: return "Host key changed"
        case .hostKeyUnconfirmed?: return "Confirm host key"
        case nil: break
        }
        #endif
        return switch state {
        case .disconnected: "Disconnected"
        case .connecting: "Connecting…"
        case .connected(.running):
            projects.isEmpty ? "Connected" : "\(projects.count) project\(projects.count == 1 ? "" : "s")"
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
    /// (`unsavedFileNames`) before an explicit disconnect. With `keepUnsaved`, projects with
    /// open edits are retained as unavailable instead of dropped.
    func disconnect(remember: Bool = false, keepUnsaved: Bool = false) {
        if remember { Self.setUserDisconnected(machine.id, true) }
        isUserDisconnecting = true
        stopRefreshing()
        let savers = removeAllProjects(keepingUnsaved: keepUnsaved)
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
        stopRefreshing()
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

    #if os(iOS)
    // MARK: - Host key (IPAD-009)

    private func applyHostKeyRefusal(_ refusal: SSHTrustError?) {
        hostKeyRefusal = refusal
        #if DEBUG
        if case .hostKeyUnconfirmed? = refusal, DebugLaunchArguments.current.trustHostKey {
            confirmHostKey()
        }
        #endif
    }

    /// The user confirmed the host key the machine presented on first use: pin it and connect.
    func confirmHostKey() {
        guard case .hostKeyUnconfirmed(let key)? = hostKeyRefusal else { return }
        do {
            try HostKeyPinStore.shared.pin(key, machineId: machine.id)
        } catch {
            actionError = "Could not trust the host key of \(machine.name): \(error.localizedDescription)"
            return
        }
        hostKeyRefusal = nil
        reconnectNow()
    }

    /// Reset Host Key: forget the pin so the next connection asks for confirmation again. Only
    /// a deliberate user action calls this; a changed key never clears its pin by itself.
    func resetHostKey() {
        do {
            try HostKeyPinStore.shared.reset(machineId: machine.id)
        } catch {
            actionError = "Could not reset the host key of \(machine.name): \(error.localizedDescription)"
            return
        }
        hostKeyRefusal = nil
        reconnectNow()
    }

    /// Connect again at once (a trusted or reset host key, an imported identity).
    func reconnectNow() {
        Self.setUserDisconnected(machine.id, false)
        isUserDisconnecting = false
        let connection = self.connection!
        let pending = disconnectTask
        Task {
            await pending?.value
            await connection.reconnectNow()
        }
    }

    /// Back from the background: check the SSH connection still answers, and replace it if
    /// not (`MachineConnection.checkAfterForeground`).
    func checkAfterForeground() {
        let connection = self.connection!
        Task { await connection.checkAfterForeground() }
    }
    #endif

    private func apply(_ newState: MachineConnectionState) {
        // Reports still in flight from a connection the user closed must not reopen it.
        if isUserDisconnecting, newState != .disconnected { return }
        let wasRunning = state.isDaemonRunning
        state = newState
        if newState.isDaemonRunning {
            if !wasRunning { startRefreshing() }
        } else {
            stopRefreshing()
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
        stopRefreshing()
        let subscription = DaemonProjectsSubscription(client: endpoint.makeClient())
        projectsSubscription = subscription
        Task {
            await subscription.start { [weak self] roots in
                guard let self, self.projectsSubscription === subscription, self.state.isDaemonRunning else { return }
                self.setProjects(roots: roots)
            }
        }
        refreshTask = Task { [weak self] in
            while !Task.isCancelled {
                #if os(iOS)
                // Paused in the background; the return to the foreground refreshes at once.
                await AppActivity.shared.untilForeground()
                if Task.isCancelled { return }
                #endif
                await self?.refreshProjects()
                do { try await Task.sleep(for: Self.refreshInterval) } catch { return }
            }
        }
    }

    private func stopRefreshing() {
        refreshTask?.cancel()
        refreshTask = nil
        let subscription = projectsSubscription
        projectsSubscription = nil
        Task { await subscription?.stop() }
    }

    /// Discover the daemon's registered projects (`inventory`, kind projects, all roots).
    func refreshProjects() async {
        let client = endpoint.makeClient()
        let revision = orderRevision
        let response: DaemonResponse
        do {
            response = try await client.send(.inventory(kind: "projects"))
        } catch {
            if state.isDaemonRunning {
                actionError = "Could not list projects on \(machine.name): \(error.localizedDescription)"
            }
            return
        }
        guard state.isDaemonRunning else { return }
        switch response {
        case .inventory(let report):
            actionError = nil
            // Listed before a move this app made: the next refresh brings the saved order.
            guard revision == orderRevision else { return }
            setProjects(roots: report.projects.map(\.projectRoot))
        case .error(_, let message):
            actionError = "Could not list projects on \(machine.name): \(message)"
        default:
            break
        }
    }

    private func setProjects(roots: [String]) {
        var visible = roots.filter { !hiddenRoots.contains($0) }
        if orderSaver.pending > 0 {
            // The list may predate a move still being saved: keep the order on screen.
            visible = ProjectOrder.sorted(visible, by: projects.map(\.projectRoot), root: { $0 })
        }
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
        var seen = Set<String>()
        // One row per root: a repeated root would otherwise build two sidebar nodes with
        // the same identity, so an expansion, a lookup or a click could land on either.
        for root in visible where seen.insert(root).inserted {
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
        hasLoadedProjects = true
        for project in added { project.startWatching() }
        for project in restored {
            project.isUnavailable = false
            project.startWatching()
        }
    }

    /// Move a project to `index` among this machine's projects and save the order on the
    /// machine, so every device lists its projects the same way.
    func moveProject(root: String, to index: Int) {
        guard let next = ProjectOrder.moving(root, to: index, in: projects) else { return }
        projects = next
        orderRevision += 1
        orderSaver.save(next.map(\.projectRoot)) { [weak self] in
            guard let self else { return }
            Task {
                // Show the order the machine kept.
                await self.refreshProjects()
                self.actionError = "Could not save the project order on \(self.machine.name)."
            }
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

    /// Hide a project on this device only. Nothing on the machine is deleted or stopped, and
    /// other devices still show it; `removeProject` removes it from the machine instead.
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

    /// Show a hidden project again; it reappears with the next project refresh.
    func unhideProject(root: String) {
        guard hiddenRoots.remove(root) != nil else { return }
        hiddenCountRevision += 1
        UserDefaults.standard.set(Array(hiddenRoots), forKey: Self.hiddenKey(machine.id))
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
