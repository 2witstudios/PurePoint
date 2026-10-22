#if os(macOS)
import AppKit
#else
import SwiftUI
import UIKit
#endif
import CryptoKit
import Foundation
import Observation

/// Multi-project container. Holds an array of ProjectState — one per open project.
/// Provides cross-project queries and manages global daemon lifecycle.
@Observable
@MainActor
final class AppState {
    /// Local projects, in sidebar order. Remote projects belong to their machine.
    var projects: [ProjectState] = []
    /// Machines reachable over SSH, each with the projects its daemon has registered.
    var machines: [MachineState] = []
    /// Folder-backed daemon state for Point Guard, kept out of the project sidebar.
    private var pointGuardProjects: [String: ProjectState] = [:]
    /// Ownership survives project promotion and lazy workspace restoration.
    private var pointGuardFolders = Set<String>()
    /// Unknown until Pi reports its actual configured folder.
    var pointGuardRoot: String?

    private var localManagedProjects: [ProjectState] {
        projects + pointGuardProjects.values.filter { hidden in
            !projects.contains { $0.projectRoot == hidden.projectRoot }
        }
    }

    /// Every project the app follows: local, Point Guard folders, and each machine's.
    private var managedProjects: [ProjectState] {
        localManagedProjects + remoteProjects
    }

    var remoteProjects: [ProjectState] { machines.flatMap(\.projects) }

    /// The active project's key (`ProjectKey.id`) — its root for local projects.
    var activeProjectRoot: String?

    /// The active project when it is local. Library screens (agent definitions, schedules,
    /// triggers, agent settings) talk to the local daemon only.
    #if os(macOS)
    var activeLocalProjectRoot: String? {
        guard let activeProjectRoot, !ProjectKey(id: activeProjectRoot).isRemote else {
            return projects.first?.projectRoot
        }
        return activeProjectRoot
    }
    #else
    /// On iPad the library screens talk to the library machine's daemon (IPAD-012), so this is
    /// the active project's root on that machine, or that machine's first project.
    var activeLocalProjectRoot: String? {
        guard let machine = libraryMachine else { return nil }
        if let activeProjectRoot {
            let key = ProjectKey(id: activeProjectRoot)
            if key.machineId == machine.machine.id { return key.root }
        }
        return machine.projects.first?.projectRoot
    }
    #endif

    /// The project roots the library screens list and create in.
    var libraryProjectRoots: [String] {
        #if os(macOS)
        projects.map(\.projectRoot)
        #else
        libraryMachine?.projects.map(\.projectRoot) ?? []
        #endif
    }
    // ContentView starts on Dashboard before its first selection-change callback.
    var activeSidebarSelection: SidebarSelection? = .nav(.dashboard)
    var daemonError: String?
    var showSettings = false
    /// Agent ID a spawn just returned. Resolved to its workspace once the manifest lands.
    var pendingSelectAgentId: String?
    var pendingSelectWorktreeId: String?
    /// Workspace to select once the sidebar has it — used to restore the last session.
    var pendingSelectWorkspaceId: String?
    var pendingSignalDestination: AgentSignalDestination?
    var pendingFocusAgentId: String?

    @ObservationIgnored lazy var pointGuardChat = PiChatModel()
    #if os(macOS)
    @ObservationIgnored lazy var pointGuardService = PointGuardServiceModel()
    #endif

    #if os(macOS)
    var agentsHubState = AgentsHubState()
    var agentConfigState = AgentConfigState()
    var scheduleState = ScheduleState()
    var triggersState = TriggersState()
    #else
    /// The machine the library screens use when the selection names none (IPAD-012).
    var pickedLibraryMachineId: UUID?
    /// Each machine's library states, made when its library is first shown
    /// (`AppState+LibraryScope+iOS.swift`).
    @ObservationIgnored var libraryStatesByMachine: [UUID: LibraryStates] = [:]
    @ObservationIgnored lazy var noMachineLibraryStates = LibraryStates(endpoint: nil)
    #endif

    weak var registry: WorkspaceRegistry?

    /// The agent in the focused pane of the on-screen workspace, or nil.
    var focusedAgentId: String? { registry?.activeWorkspace?.focusedAgentId }

    @ObservationIgnored private let service: any WorkspaceService
    @ObservationIgnored private let startProject: @MainActor (ProjectState) -> Void
    /// Whether `restoreProjects()` follows the local daemon's project list (off in tests).
    @ObservationIgnored private let followsLocalDaemon: Bool
    #if os(macOS)
    @ObservationIgnored private var binaryWatcher: ManifestWatcher?
    #endif
    @ObservationIgnored private var machineRegistryWatcher: MachineRegistryWatcher?
    #if os(macOS)
    /// Follows the local daemon's registered projects, which decide this Mac's sidebar.
    @ObservationIgnored private var localProjectsSubscription: DaemonProjectsSubscription?
    /// The local daemon's projects as last reported, to tell a removal from a project
    /// whose `init` has not reached the daemon yet; nil until the first report.
    @ObservationIgnored private var lastLocalProjects: Set<String>?
    /// Each opened root's canonical path, resolved once rather than on every report.
    @ObservationIgnored private var canonicalRoots: [String: String] = [:]
    /// The local daemon's project order as last reported.
    @ObservationIgnored private var localProjectOrder: [String] = []
    @ObservationIgnored private lazy var localOrderSaver = ProjectOrderSaver(endpoint: .local)
    #endif
    /// Point Guard folders the user also opened as sidebar projects. Other Point Guard
    /// folders stay out of the sidebar even though the daemon has them registered.
    @ObservationIgnored private var shownPointGuardFolders = Set<String>()
    /// Whether the first local project list reopens last session's sidebar, whose agents
    /// this app suspended on quit and resumes. Projects that join later keep theirs paused.
    @ObservationIgnored private var resumesOnFirstLocalList = false

    /// The sidebar list from before the daemon owned it; read once, then deleted.
    private static let openProjectsKey = "PurePointOpenProjects"
    private static let shownPointGuardFoldersKey = "PurePointShownPointGuardFolders"

    init(
        service: any WorkspaceService = DaemonWorkspaceService(),
        startProject: @escaping @MainActor (ProjectState) -> Void = { $0.startWatching() },
        followsLocalDaemon: Bool = true
    ) {
        self.service = service
        self.startProject = startProject
        self.followsLocalDaemon = followsLocalDaemon
        #if os(iOS)
        installLifecycleHooks()
        #endif
    }

    var isLoaded: Bool { !projects.isEmpty }

    // Freshness advances even when no new status packet arrives.
    var signalClock = Date()
    @ObservationIgnored private lazy var signalNotifications = AgentSignalNotifications.shared

    var signalRoutingTargets: [AgentSignalRouting.Target] {
        guard let registry else { return [] }
        return registry.workspacesByProject.flatMap { projectID, workspaces in
            workspaces.enumerated().flatMap { ordinal, workspace in
                workspace.surfaces.map { entry in
                    AgentSignalRouting.Target(projectID: projectID, worktreeID: workspace.container.worktreeId,
                        workspaceID: workspace.id, agentID: entry.surface.content.agentId,
                        leafID: entry.leafId, surfaceID: entry.surface.id, workspaceOrdinal: ordinal)
                }
            }
        }
    }

    private var visibleSignalWorkspaceID: String? {
        switch activeSidebarSelection {
        case .workspace(let id): return id
        case .nav(.dashboard) where registry?.activeWorkspace?.isPointGuard == true:
            return registry?.activeWorkspaceId
        default: return nil
        }
    }

    private func signalIdentity(projectID: String, worktreeID: String?, agentID: String) -> String {
        let data = (try? JSONEncoder().encode([projectID, worktreeID, agentID])) ?? Data()
        return SHA256.hash(data: data).map { String(format: "%02x", $0) }.joined()
    }

    func updateAgentSignals() {
        signalClock = Date()
        var foreground: String?
        let targets = signalRoutingTargets
        let agents = managedProjects.flatMap { project in
            project.allAgents.map { agent -> AgentSignalAlertPolicy.Agent in
                let worktreeID = project.worktreeId(forAgentId: agent.id)
                let identity = self.signalIdentity(projectID: project.projectId, worktreeID: worktreeID, agentID: agent.id)
                let presentation = agent.signalPresentation(available: !project.isUnavailable, now: signalClock)
                let destination = agent.signal.flatMap { signal -> AgentSignalDestination? in
                    guard let event = presentation.eventID else { return nil }
                    let destination = AgentSignalDestination(projectID: project.projectId, worktreeID: worktreeID,
                        agentID: agent.id, launchID: signal.identity.launch_id, generation: signal.identity.generation,
                        nativeSessionID: signal.identity.native_session_id, eventID: event)
                    return destination.matches(signal, now: signalClock)
                        && AgentSignalRouting.target(for: destination, among: targets) != nil ? destination : nil
                }
                if let destination, let target = AgentSignalRouting.target(for: destination, among: targets),
                    target.workspaceID == visibleSignalWorkspaceID,
                    AgentSignalPresence.isVisible(appActive: NSApplication.shared.isActive,
                        keyWindow: NSApplication.shared.keyWindow != nil, settingsVisible: showSettings,
                        selectedWorkspaceID: visibleSignalWorkspaceID, activeWorkspaceID: registry?.activeWorkspaceId,
                        focusedAgentID: focusedAgentId, activeProjectID: activeProjectRoot, destination: destination) {
                    foreground = identity
                }
                return .init(identity: identity, launch: agent.signal?.identity.launch_id ?? "",
                    presentation: presentation, qualificationKey: agent.signal?.qualificationKey,
                    macOSActionable: agent.signal?.pending_attention.contains {
                        $0.event_id == presentation.eventID && $0.actionability?.macos == true
                    } ?? false, destination: destination)
            }
        }
        signalNotifications.open = { [weak self] notification in
            guard let self, let destination = notification.destination,
                let project = self.managedProjects.first(where: { $0.projectId == destination.projectID }),
                let agent = project.agent(byId: destination.agentID),
                destination.matches(agent.signal), agent.status.isAlive, !agent.suspended, !project.isUnavailable,
                AgentSignalRouting.target(for: destination, among: self.signalRoutingTargets) != nil
            else { return false }
            guard let response = try? await project.client.send(.status(projectRoot: project.projectRoot)),
                case .statusReport(let worktrees, let roots) = response else { return false }
            let current: AgentSignalSnapshot?
            if let worktreeID = destination.worktreeID {
                let matching = worktrees.filter { $0.id == worktreeID }
                guard matching.count == 1 else { return false }
                guard let entry = matching[0].agents[destination.agentID], entry.status.isAlive,
                    entry.suspended != true else { return false }
                current = entry.signal
            } else {
                let matching = roots.filter { $0.id == destination.agentID }
                guard matching.count == 1 else { return false }
                guard matching[0].status == "running", !matching[0].suspended else { return false }
                current = matching[0].signal
            }
            guard destination.matches(current),
                self.managedProjects.contains(where: { $0 === project }), !project.isUnavailable,
                AgentSignalRouting.target(for: destination, among: self.signalRoutingTargets) != nil else { return false }
            project.refresh()
            self.pendingSignalDestination = destination
            NSApplication.shared.activate(ignoringOtherApps: true)
            return true
        }
        signalNotifications.update(agents, foregroundIdentity: foreground)
    }

    // MARK: - Project Management

    /// Open a project on this Mac. iOS has no local machine: every iPad project is
    /// machine-backed and arrives through its `MachineState`, so a local root never opens.
    func openProject(_ root: String, resumingSuspendedAgents: Bool = true) {
        #if os(macOS)
        guard !projects.contains(where: { $0.projectRoot == root }) else { return }

        let project = pointGuardProjects.removeValue(forKey: root)
            ?? ProjectState(projectRoot: root, service: service, registry: registry)
        project.appState = self
        if !resumingSuspendedAgents { project.keepsSuspendedAgentsOnNextOpen = true }
        projects.append(project)
        if pointGuardFolders.contains(root) { shownPointGuardFolders.insert(root) }
        startProject(project)
        initializeActiveProjectIfNeeded(root: root)

        startBinaryWatcherIfNeeded()

        persistOpenProjects()
        #endif
    }

    /// Returns true when the root is no longer managed and its local files can be discarded.
    @discardableResult
    func closeProject(_ root: String) -> Bool {
        let key = ProjectKey(id: root)
        if key.isRemote {
            // Removing a remote project only hides it here; nothing on the machine changes.
            guard let machine = machines.first(where: { $0.machine.id == key.machineId }) else { return false }
            machine.hideProject(root: key.root)
            if activeProjectRoot == root { activeProjectRoot = projects.first?.projectRoot }
            return false
        }
        guard let index = projects.firstIndex(where: { $0.projectRoot == root }) else { return false }
        let project = projects.remove(at: index)
        if isPointGuardFolder(root) {
            pointGuardFolders.insert(root)
            shownPointGuardFolders.remove(root)
            pointGuardProjects[root] = project
        } else {
            project.stopWatching()
            registry?.forgetProject(root)
        }
        if activeProjectRoot == root {
            activeProjectRoot = projects.first?.projectRoot
        }
        persistOpenProjects()
        return projectState(forRoot: root) == nil
    }

    /// Whether Point Guard uses this local folder, so it must stay registered and followed
    /// even when it is not in the sidebar.
    func isPointGuardFolder(_ root: String) -> Bool {
        root == pointGuardRoot || pointGuardFolders.contains(root)
            || registry?.workspacesByProject[root]?.contains(where: \.isPointGuard) == true
    }

    /// Remove a project for every device that shows it: its machine's daemon forgets it
    /// (`remove_project`), suspending its running agents and keeping its files, and each
    /// device's sidebar follows. A local folder Point Guard still uses only leaves this
    /// Mac's sidebar, since Point Guard needs it registered.
    func removeProject(_ project: ProjectState) async throws {
        if project.isRemote {
            guard let machine = machine(for: project) else { return }
            try await machine.removeProject(root: project.key.root, suspendAgents: true)
            if activeProjectRoot == project.projectId { activeProjectRoot = projects.first?.projectRoot }
            return
        }
        let root = project.projectRoot
        if !isPointGuardFolder(root) {
            // The daemon registered the canonical path; a symlinked one may name a deleted folder.
            try await DaemonEndpoint.local.makeClient().removeProject(
                root: Self.canonicalPath(root), suspendAgents: true, daemonName: "The daemon")
        }
        closeProject(root)
    }

    /// Whether a path is too broad to open as a project: `/`, `$HOME`, or an ancestor
    /// of `$HOME`. Mirrors the daemon's `is_too_broad_project_root` validation so the
    /// app doesn't attempt to open roots the daemon will reject.
    func isTooBroadProjectRoot(_ path: String) -> Bool {
        let url = URL(fileURLWithPath: path)
        let canonical = url.resolvingSymlinksInPath().standardizedFileURL.path
        // Filesystem root
        if canonical == "/" { return true }
        // $HOME or an ancestor of $HOME
        if let home = ProcessInfo.processInfo.environment["HOME"] {
            let homeCanonical = URL(fileURLWithPath: home)
                .resolvingSymlinksInPath().standardizedFileURL.path
            if homeCanonical.hasPrefix(canonical + "/") || homeCanonical == canonical {
                return true
            }
        }
        return false
    }

    // MARK: - Project Order

    /// The projects listed beside `project`: the local ones, or its machine's.
    func siblingProjects(of project: ProjectState) -> [ProjectState] {
        guard let machineId = project.key.machineId else { return projects }
        return machines.first { $0.machine.id == machineId }?.projects ?? []
    }

    /// Move a project to `index` among its siblings. The order is saved on the machine that
    /// owns the project, so every device lists it the same way.
    func moveProject(_ project: ProjectState, to index: Int) {
        if let machine = machine(for: project) {
            machine.moveProject(root: project.projectRoot, to: index)
            return
        }
        #if os(macOS)
        guard let next = ProjectOrder.moving(project.projectRoot, to: index, in: projects) else { return }
        projects = next
        // On success the daemon pushes the new order to every device following it.
        localOrderSaver.save(next.map(\.projectRoot)) { [weak self] in
            guard let self else { return }
            self.daemonError = "Could not save the project order."
            self.sortLocalProjects(by: self.localProjectOrder)
        }
        #endif
    }

    /// Open the projects this Mac shows. With a root (`--project-root`) only that one; otherwise
    /// the local daemon's registered projects, followed from then on, so a project added or
    /// removed on any device, or with `pu projects`, is added or removed here too.
    func restoreProjects(projectRoot: String? = nil) {
        // The list saved before the daemon owned it is opened once, which registers each
        // project with the daemon, and then deleted.
        let paths = projectRoot.map { [$0] }
            ?? UserDefaults.standard.stringArray(forKey: Self.openProjectsKey) ?? []
        // Opening projects persists current state, so snapshot the saved lists first.
        let savedPointGuardFolders = UserDefaults.standard.stringArray(forKey: "PurePointPointGuardFolders") ?? []
        pointGuardFolders.formUnion(savedPointGuardFolders)
        shownPointGuardFolders.formUnion(
            UserDefaults.standard.stringArray(forKey: Self.shownPointGuardFoldersKey) ?? [])
        var notImported: [String] = []
        for path in paths {
            guard projectRoot != nil || FileManager.default.fileExists(atPath: path) else {
                // On a volume that is not mounted yet: try again next launch.
                notImported.append(path)
                continue
            }
            guard !isTooBroadProjectRoot(path) else { continue }
            openProject(path)
        }
        // The first list reopens last session's sidebar, unless that came from the legacy list.
        resumesOnFirstLocalList = paths.count == notImported.count
        if projectRoot == nil {
            if notImported.isEmpty {
                UserDefaults.standard.removeObject(forKey: Self.openProjectsKey)
            } else {
                UserDefaults.standard.set(notImported, forKey: Self.openProjectsKey)
            }
        }
        for root in savedPointGuardFolders {
            guard FileManager.default.fileExists(atPath: root) else { continue }
            guard !isTooBroadProjectRoot(root) else { continue }
            ensurePointGuardProject(root: root)
        }
        #if os(macOS)
        if projectRoot == nil, followsLocalDaemon { followLocalProjects() }
        #endif
        // Workspace layouts load lazily in WorkspaceRegistry.reconcile, on the first
        // manifest read for each project.
    }

    #if os(macOS)
    private func followLocalProjects() {
        guard localProjectsSubscription == nil else { return }
        let subscription = DaemonProjectsSubscription(client: DaemonEndpoint.local.makeClient())
        localProjectsSubscription = subscription
        Task { [weak self] in
            // The daemon holds the project list, so it runs even before any project opens.
            try? await DaemonLifecycle.ensureDaemon()
            await subscription.start { [weak self] roots in self?.applyLocalProjects(roots) }
        }
    }
    #endif

    /// Match the sidebar to the local daemon's projects. A project that left the list was
    /// removed (on another device, or with `pu projects remove`) and closes here; one that
    /// joined opens, unless it is a Point Guard folder the user has not opened.
    func applyLocalProjects(_ roots: [String]) {
        #if os(macOS)
        let registered = Set(roots)
        // The daemon reports canonical roots; a project may have been opened by another path.
        let canonical = Dictionary(
            projects.map { (canonicalRoot(of: $0.projectRoot), $0.projectRoot) }, uniquingKeysWith: { first, _ in first })
        let pointGuard = Dictionary(
            pointGuardProjects.map { (canonicalRoot(of: $0.key), $0.value) }, uniquingKeysWith: { first, _ in first })
        for root in (lastLocalProjects ?? []).subtracting(registered) {
            if let owner = canonical[root].flatMap({ projectState(forRoot: $0) }) ?? pointGuard[root],
                isPointGuardFolder(owner.projectRoot)
            {
                // Removed elsewhere, but Point Guard works in this folder: register it again,
                // leaving the agents the removal suspended paused.
                owner.keepsSuspendedAgentsOnNextOpen = true
                startProject(owner)
            } else if let open = canonical[root] {
                closeProject(open)
            }
        }
        let resuming = resumesOnFirstLocalList
        resumesOnFirstLocalList = false
        for root in roots where canonical[root] == nil {
            // A Point Guard folder the user has not opened stays out of the sidebar.
            if let owner = pointGuard[root], !shownPointGuardFolders.contains(owner.projectRoot) { continue }
            guard FileManager.default.fileExists(atPath: root), !isTooBroadProjectRoot(root) else { continue }
            openProject(root, resumingSuspendedAgents: resuming)
        }
        // The daemon's list is in its saved order, which every device shows, unless it may
        // predate a move this Mac is still saving.
        localProjectOrder = roots
        if localOrderSaver.pending == 0 { sortLocalProjects(by: roots) }
        lastLocalProjects = registered
        #endif
    }

    #if os(macOS)
    private func sortLocalProjects(by roots: [String]) {
        let ordered = ProjectOrder.sorted(projects, by: roots, root: { self.canonicalRoot(of: $0.projectRoot) })
        if ordered.map(\.projectRoot) != projects.map(\.projectRoot) { projects = ordered }
    }
    #endif

    #if os(macOS)
    private func canonicalRoot(of root: String) -> String {
        if let known = canonicalRoots[root] { return known }
        guard let resolved = Self.resolvedPath(root) else {
            // Not resolvable yet (an unmounted volume): try again on the next report.
            return root
        }
        canonicalRoots[root] = resolved
        return resolved
    }
    #endif

    /// Until the local daemon first reports its projects, a saved workspace selection may
    /// belong to one that has not opened yet.
    private var awaitingLocalProjects: Bool {
        #if os(macOS)
        localProjectsSubscription != nil && lastLocalProjects == nil
        #else
        false
        #endif
    }

    /// The machine a remote project lives on; nil for a local one.
    func machine(for project: ProjectState) -> MachineState? {
        machines.first { $0.machine.id == project.key.machineId }
    }

    /// The path with symlinks resolved, as the daemon's `canonicalize` reports it (unlike
    /// `URL.resolvingSymlinksInPath`, which drops `/private`).
    nonisolated static func canonicalPath(_ path: String) -> String {
        resolvedPath(path) ?? path
    }

    /// The path with symlinks resolved, or nil when it does not resolve (yet).
    nonisolated private static func resolvedPath(_ path: String) -> String? {
        guard let resolved = realpath(path, nil) else { return nil }
        defer { free(resolved) }
        return String(cString: resolved)
    }

    // MARK: - Machines

    /// Read `~/.pu/machines.json` and connect every machine the user has not disconnected.
    /// Starts a file watcher so later CLI or other-instance edits show up without a restart.
    func loadMachines() {
        FileIOService.removeLocalCopies()
        reloadMachines()
        startMachineRegistryWatcher()
    }

    /// Re-read `~/.pu/machines.json` and diff against the current machines: connect any new
    /// ones, update existing ones whose name or SSH host changed, and remove any that are no
    /// longer registered. Called at startup and by the registry file watcher.
    func reloadMachines() {
        let registered: [Machine]
        do {
            registered = try MachineRegistryStore.load()
        } catch {
            daemonError = "Could not read machines from \(MachineRegistryStore.displayPath): \(error.localizedDescription)"
            return
        }
        for machine in registered {
            if let existing = machines.first(where: { $0.machine.id == machine.id }) {
                existing.update(from: machine)
            } else {
                let state = MachineState(machine: machine, appState: self, registry: registry)
                machines.append(state)
                if !MachineState.isUserDisconnected(machine.id) { state.connect() }
            }
        }
        // Remove machines no longer in the registry (CLI remove or another instance).
        // KeepUnsaved preserves projects with unsaved tabs as unavailable.
        let registeredIds = Set(registered.map(\.id))
        let removed = machines.filter { !registeredIds.contains($0.machine.id) }
        for state in removed {
            state.disconnect(keepUnsaved: true)
            machines.removeAll { $0 === state }
            if let active = activeProjectRoot, ProjectKey(id: active).machineId == state.machine.id {
                activeProjectRoot = projects.first?.projectRoot
            }
        }
    }

    /// Watch `~/.pu/machines.json` for changes from the CLI or another app instance.
    private func startMachineRegistryWatcher() {
        guard machineRegistryWatcher == nil else { return }
        machineRegistryWatcher = MachineRegistryWatcher { [weak self] in
            self?.reloadMachines()
        }
    }

    /// The registry update takes a file lock shared with `pu machine`, so it runs off the
    /// main thread.
    @discardableResult
    func addMachine(name: String, sshHost: String, id: UUID = UUID()) async throws -> MachineState {
        let machine = try await Task.detached {
            try MachineRegistryStore.update { machines in
                try MachineRegistryStore.add(name: name, sshHost: sshHost, id: id, to: &machines)
            }
        }.value
        // The watcher may have reloaded between the registry write and this resume, adding
        // this machine already; reuse that state rather than creating a duplicate.
        if let existing = machines.first(where: { $0.machine.id == machine.id }) {
            existing.update(from: machine)
            if !MachineState.isUserDisconnected(machine.id) { existing.connect() }
            return existing
        }
        let state = MachineState(machine: machine, appState: self, registry: registry)
        machines.append(state)
        state.connect()
        return state
    }

    /// Forget a machine: close its forward and drop it from the shared registry. Its daemon,
    /// projects and agents on the machine are untouched.
    func removeMachine(_ state: MachineState) {
        state.disconnect()
        machines.removeAll { $0 === state }
        let id = state.machine.id
        let name = state.machine.name
        #if os(iOS)
        // A removed machine's host key is trusted no longer (REQ-IPAD-026).
        try? HostKeyPinStore.shared.reset(machineId: id)
        #endif
        Task {
            do {
                try await Task.detached {
                    try MachineRegistryStore.update { machines in machines.removeAll { $0.id == id } }
                }.value
            } catch {
                daemonError = "Could not remove \(name) from \(MachineRegistryStore.displayPath): \(error.localizedDescription)"
            }
        }
        if let active = activeProjectRoot, ProjectKey(id: active).machineId == id {
            activeProjectRoot = projects.first?.projectRoot
        }
    }

    // MARK: - Active Project Routing

    /// Update `activeProjectRoot` and `activeSidebarSelection` based on what the user selected
    /// in the sidebar. Terminal, agent, worktree, and project selections resolve to their owning
    /// project; nav and nil selections preserve the last-known project.
    func updateActiveProject(for selection: SidebarSelection?) {
        activeSidebarSelection = selection

        switch selection {
        case .workspace(let id):
            if registry?.workspace(id: id)?.isPointGuard != true,
                let root = registry?.projectRoot(forWorkspace: id) {
                activeProjectRoot = root
            }
        case .worktree(let id):
            if let root = projectState(forWorktreeId: id)?.projectId {
                activeProjectRoot = root
            }
        case .project(let root), .channel(let root):
            activeProjectRoot = root
        case nil, .nav:
            break  // keep last known project
        }
    }

    /// Set `activeProjectRoot` to the given root if no project is active yet.
    /// Called when a project is first opened so Cmd+N works before any sidebar click.
    func initializeActiveProjectIfNeeded(root: String) {
        if activeProjectRoot == nil {
            activeProjectRoot = root
        }
    }

    /// Pi's configured folder is independent of the selected coding project.
    func updatePointGuardRoot(_ cwd: String) {
        pointGuardRoot = cwd.isEmpty ? nil : URL(fileURLWithPath: cwd).standardizedFileURL.path
    }

    private func ensurePointGuardProject(root: String) {
        pointGuardFolders.insert(root)
        if projects.contains(where: { $0.projectRoot == root }) { shownPointGuardFolders.insert(root) }
        persistOpenProjects()
        #if os(macOS)
        guard projectState(forRoot: root) == nil else { return }
        let project = ProjectState(projectRoot: root, service: service, registry: registry)
        project.appState = self
        pointGuardProjects[root] = project
        // Broad roots have no ProjectState owner for routing; skip daemon registration.
        guard !isTooBroadProjectRoot(root) else { return }
        startProject(project)
        startBinaryWatcherIfNeeded()
        #endif
    }

    @discardableResult
    func activatePointGuardWorkspace() -> String? {
        guard let registry else { return nil }
        guard let root = pointGuardRoot else {
            registry.activeWorkspaceId = nil
            return nil
        }
        guard !isTooBroadProjectRoot(root) else {
            registry.activeWorkspaceId = nil
            return nil
        }
        ensurePointGuardProject(root: root)
        guard let id = try? registry.ensurePointGuardWorkspace(projectRoot: root) else {
            registry.activeWorkspaceId = nil
            return nil
        }
        registry.activate(workspaceId: id)
        return id
    }

    /// Cmd+N uses the same folder and palette as Point Guard splits.
    func newPointGuardTab() {
        guard let registry, let id = activatePointGuardWorkspace(),
            let workspace = registry.workspace(id: id)
        else { return }
        registry.pendingPaletteSurfaceId = registry.newTab(workspaceId: id, leafId: workspace.focusedLeafId)
    }

    /// Load the pane's own local definitions as well as definitions from visible projects.
    /// The owning root comes first so a Point Guard-only session still queries global definitions.
    func paletteProjectRoots(forWorkspace workspaceId: String) -> [String] {
        let visibleRoots = projects.map(\.projectRoot)
        // Definitions come from the local daemon, so a remote workspace adds no root of its own.
        guard let root = registry?.projectRoot(forWorkspace: workspaceId), !ProjectKey(id: root).isRemote else {
            return visibleRoots
        }
        return [root] + visibleRoots.filter { $0 != root }
    }

    // MARK: - Cross-Project Queries

    func agent(byId id: String) -> AgentModel? {
        for project in managedProjects {
            if let agent = project.agent(byId: id) { return agent }
        }
        return nil
    }

    func projectState(forAgentId agentId: String) -> ProjectState? {
        managedProjects.first { $0.agent(byId: agentId) != nil }
    }

    func projectState(forWorktreeId worktreeId: String) -> ProjectState? {
        managedProjects.first { $0.worktrees.contains { $0.id == worktreeId } }
    }

    /// The project with this key (`ProjectKey.id`; a local project's id is its root).
    func projectState(forId id: String) -> ProjectState? {
        managedProjects.first { $0.projectId == id }
    }

    func projectState(forRoot root: String) -> ProjectState? { projectState(forId: root) }

    func projectState(forWorkspace workspaceId: String, registry: WorkspaceRegistry) -> ProjectState? {
        registry.projectRoot(forWorkspace: workspaceId).flatMap { projectState(forId: $0) }
    }

    func agentId(forSessionId sessionId: String) -> String? {
        for project in managedProjects {
            if let agent = project.allAgents.first(where: { $0.sessionId == sessionId }) {
                return agent.id
            }
        }
        return nil
    }

    func worktreeId(forPath path: String) -> String? {
        let normalizedPath = URL(fileURLWithPath: path).standardizedFileURL.path
        for project in managedProjects {
            if let worktree = project.worktrees.first(where: {
                URL(fileURLWithPath: $0.path).standardizedFileURL.path == normalizedPath
            }) {
                return worktree.id
            }
        }
        return nil
    }

    // MARK: - Lifecycle

    #if os(macOS)
    /// Synchronously suspend all agents and shut down daemon.
    /// Must complete before the process exits — uses DispatchSemaphore to block.
    func shutdownWithSuspend() {
        pointGuardService.stopForTermination()
        persistSelectedAgent()
        registry?.saveAll()

        for project in managedProjects {
            project.stopWatching()
        }
        // Remote daemons own their agents (MACH-004): only the SSH forwards close — after
        // the layout saves saveAll just queued have gone through them (2 s at most overall).
        let remoteSaveDeadline = DispatchTime.now() + 2.0
        for machine in machines {
            machine.disconnectForTermination(deadline: remoteSaveDeadline)
        }
        binaryWatcher?.stop()
        binaryWatcher = nil
        machineRegistryWatcher?.stop()
        machineRegistryWatcher = nil
        FileIOService.removeLocalCopies()

        // Block until daemon RPCs complete (macOS terminates shortly after willTerminate)
        let projectRoots = localManagedProjects.map(\.projectRoot)
        let semaphore = DispatchSemaphore(value: 0)
        Task.detached {
            let client = DaemonEndpoint.local.makeClient()
            // Only an instance that owns the daemon stops anything. A second app
            // instance attached to a shared daemon must leave the owner's agents
            // running: suspending them would strand them, since that daemon stays
            // up and nothing would resume them.
            guard await DaemonLifecycle.didLaunchDaemon() else {
                semaphore.signal()
                return
            }
            for root in projectRoots {
                _ = try? await client.send(.suspend(projectRoot: root, target: .all))
            }
            _ = try? await client.send(.shutdown)
            semaphore.signal()
        }
        // Timeout after 5s — don't hang indefinitely if daemon is unresponsive
        _ = semaphore.wait(timeout: .now() + 5.0)
    }
    #endif

    #if os(iOS)
    /// The iPad's replacement for the Mac's `NSApplication` activity and termination hooks:
    /// the scene phase the iPad App forwards. Installed when the App creates its state, which
    /// is also before launch-time notification responses arrive, as the Mac's application
    /// delegate installs the notification delegate.
    private func installLifecycleHooks() {
        _ = AgentSignalNotifications.shared
        AppLifecycleHooks.scenePhaseChanged = { [weak self] phase in
            self?.scenePhaseChanged(phase)
        }
    }

    /// Background: pause polling (pollers wait on `AppActivity`) and save what a quit would
    /// save, since iOS may end a suspended app without notice. Foreground again: refresh state
    /// and resume the daemon streams.
    func scenePhaseChanged(_ phase: ScenePhase) {
        let returned = AppActivity.shared.apply(phase)
        if phase == .background {
            persistSelectedAgent()
            registry?.saveAll()
        } else if returned {
            refreshAfterBackground()
        }
    }

    private func refreshAfterBackground() {
        for machine in machines {
            machine.checkAfterForeground()
        }
        for machine in machines where machine.state.isDaemonRunning {
            Task { await machine.refreshProjects() }
        }
        for project in managedProjects {
            project.resumeStreams()
        }
        signalNotifications.refreshBadge()
        updateAgentSignals()
    }
    #endif

    // MARK: - Selection Persistence

    private static let activeWorkspaceKey = "PurePointActiveWorkspaceId"

    private func persistSelectedAgent() {
        UserDefaults.standard.set(registry?.activeWorkspaceId, forKey: Self.activeWorkspaceKey)
    }

    /// Remember the workspace that was on screen so the UI can select it once it exists.
    /// Reconcile may not have run for every project yet, so existence is not checked here.
    @discardableResult
    func restoreActiveWorkspace() -> String? {
        guard let savedId = UserDefaults.standard.string(forKey: Self.activeWorkspaceKey) else { return nil }
        pendingSelectWorkspaceId = savedId
        return savedId
    }

    /// The pending workspace to select, once the registry has it. The pending ID is cleared
    /// when it resolves, or when every open project has reconciled and it still names nothing.
    func resolvePendingWorkspaceSelection() -> String? {
        guard let pendingId = pendingSelectWorkspaceId, let registry else { return nil }

        if let root = registry.projectRoot(forWorkspace: pendingId) {
            pendingSelectWorkspaceId = nil
            if registry.workspace(id: pendingId)?.isPointGuard != true {
                activeProjectRoot = root
            }
            return pendingId
        }
        // An unreachable machine may never load its layouts; it must not hold the selection.
        if !awaitingLocalProjects,
            localManagedProjects.allSatisfy({ registry.workspacesByProject[$0.projectId] != nil })
        {
            pendingSelectWorkspaceId = nil
        }
        return nil
    }

    // MARK: - Private

    /// The daemon keeps which projects exist; this Mac only keeps which Point Guard folders
    /// it shows.
    private func persistOpenProjects() {
        UserDefaults.standard.set(Array(shownPointGuardFolders), forKey: Self.shownPointGuardFoldersKey)
        UserDefaults.standard.set(Array(pointGuardFolders), forKey: "PurePointPointGuardFolders")
    }

    #if os(macOS)
    private func startBinaryWatcherIfNeeded() {
        guard binaryWatcher == nil, let binPath = DaemonLifecycle.findBinary() else { return }
        binaryWatcher = ManifestWatcher(path: binPath) { [weak self] in
            self?.restartDaemonAndRefresh()
        }
    }

    private func restartDaemonAndRefresh() {
        Task {
            do {
                try await DaemonLifecycle.restartDaemon()
                // Re-run the full open sequence, not just a refresh: init marks the
                // agents the old daemon stopped as suspended, and they get resumed.
                for project in localManagedProjects {
                    startProject(project)
                }
            } catch {
                self.daemonError = error.localizedDescription
            }
        }
    }
    #endif
}

extension AppState {
    /// The directory a workspace's file panes are rooted at: its worktree, else the project root.
    func fileRoot(forWorkspace workspaceId: String, registry: WorkspaceRegistry) -> String? {
        guard let projectId = registry.projectRoot(forWorkspace: workspaceId) else { return nil }
        let project = projectState(forId: projectId)
        if let worktreeId = registry.workspace(id: workspaceId)?.container.worktreeId,
            let path = project?.worktrees.first(where: { $0.id == worktreeId })?.path
        {
            return path
        }
        return project?.projectRoot ?? ProjectKey(id: projectId).root
    }

    /// Where a workspace's files live: this Mac, or its project's machine.
    func host(forWorkspace workspaceId: String, registry: WorkspaceRegistry) -> any ProjectHost {
        if let project = projectState(forWorkspace: workspaceId, registry: registry) { return project.host }
        // A remote workspace whose project is not available must never fall back to this
        // Mac's disk with the remote path.
        if let projectId = registry.projectRoot(forWorkspace: workspaceId), ProjectKey(id: projectId).isRemote {
            return UnavailableProjectHost()
        }
        #if os(macOS)
        return LocalProjectHost.shared
        #else
        // No local machine: a workspace without a project has no files to reach.
        return UnavailableProjectHost()
        #endif
    }

    /// The daemon owning an agent, so terminals attach to the right machine.
    func client(forAgentId agentId: String) -> DaemonClient? {
        projectState(forAgentId: agentId)?.client
    }
}
