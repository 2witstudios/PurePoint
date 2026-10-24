import Foundation
import SwiftUI
#if os(iOS)
import UserNotifications
#endif

@MainActor final class AgentControlModel: ObservableObject {
    typealias Request = @MainActor (String, [String: Any]) async throws -> JSONValue
    @Published private(set) var workspaceManagement = false
    @Published private(set) var workspaceSupported = false
    /// Whether the bridge can save a machine's project order.
    @Published private(set) var projectOrderSupported = false
    @Published private(set) var permission: AgentTerminalPermission = .off
    @Published private(set) var machines: [AgentMachine] = []
    @Published private(set) var connected = false
    @Published private(set) var loading = false
    @Published private(set) var selected: AgentScope?
    @Published private(set) var streamId: String?
    @Published private(set) var terminalRevision = 0
    @Published private(set) var terminalEpoch = UUID()
    @Published private(set) var cols = 80
    @Published private(set) var rows = 24
    @Published private(set) var hasSize = false
    @Published private(set) var screenMayBeInexact = false
    @Published private(set) var composer = AgentComposer()
    @Published private(set) var recoveries: [AgentPromptRecovery] = []
    @Published private(set) var attachState = AgentAttachState.idle
    /// The upload `attachState` describes; closing the agent screen lets it finish quietly.
    private var attachUpload: UUID?
    @Published var error: String?
    @Published var rawTyping = false
    @Published var rawText = ""
    @Published private(set) var navigation = AgentNavigationPreferences()
    private var host = ""
    private var request: Request?
    private var generation = 0
    private var screen = 0
    private var permissionRevision = 0
    // Workspace grant changes must not invalidate an authorized terminal receipt.
    private var terminalPermissionRevision = 0
    private var refreshRevision = 0
    /// Bumped by each project reorder; an inventory fetched across one keeps the order on screen.
    private var projectOrderRevision = 0
    /// Project reorders sent but not yet answered.
    private var projectOrdersPending = 0
    private var pendingOpen: AgentPendingOpen?
    // The agent a transport drop closed, reopened once the same host reconnects.
    private var resumeScope: AgentScope?
    private var refreshTask: Task<Void, Never>?
    private var discovering = false
    private var inventoryInFlight = false
    private var visibleRefreshTask: Task<Void, Never>?
    private var inventoryVisible = false
    private var inventoryForeground = true
    private let inventoryRefreshNanoseconds: UInt64
    private var inputRefreshTask: Task<Void, Never>?
    private var terminalChunks: [AgentTerminalUpdate] = []
    private(set) var terminalBaseCols = 80
    private(set) var terminalBaseRows = 24
    private var terminalBytes = 0
    // Last size this phone claimed for the open stream; the bridge re-asserts it on reattach.
    private var claimedSize: (cols: Int, rows: Int, stream: String)?
    private var savedRecoveries: [String: [AgentPromptRecovery]] = [:]
    private var recoveryUnavailable = false
    private var drafts: [String: String]
    private var composers: [AgentScope: AgentComposer] = [:]
    private let defaults: UserDefaults

    init(defaults: UserDefaults = .standard, inventoryRefreshNanoseconds: UInt64 = 5_000_000_000) {
        self.inventoryRefreshNanoseconds = inventoryRefreshNanoseconds
        self.defaults = defaults
        if let data = defaults.data(forKey: "pi.agentNavigation.v1"), let saved = try? JSONDecoder().decode(AgentNavigationPreferences.self, from: data) { navigation = saved }
        drafts = defaults.dictionary(forKey: "pi.agentDrafts.v1") as? [String: String] ?? [:]
        if let data = defaults.data(forKey: "pi.agentRecovery.v1") {
            do { savedRecoveries = try JSONDecoder().decode([String: [AgentPromptRecovery]].self, from: data) } catch {
                recoveryUnavailable = true;
                self.error =
                    "Saved agent prompt recovery could not be read. Keep this phone's local data and resolve storage before sending."
            }
        }
    }

    var signalAttentionCount: Int {
        guard connected, inventoryForeground else { return 0 }
        return Set(machines.flatMap { machine in
            machine.agents.compactMap { agent -> String? in
                guard machine.online, machine.metadataStatus != "error",
                    agent.signal?.identity.worktree_id == agent.worktreeId,
                    agent.signalPresentation().needsAttention else { return nil }
                return AgentScope(host: host, machine: machine.id, agent: agent.id).storageKey
            }
        }).count
    }
    private var lastSignalBadge: Int?
    func updateSignalBadge() {
        let count = signalAttentionCount
        guard lastSignalBadge != count else { return }
        lastSignalBadge = count
        #if os(iOS)
        Task { @MainActor [weak self] in
            let center = UNUserNotificationCenter.current()
            if count > 0 {
                guard (try? await center.requestAuthorization(options: [.badge])) == true else { return }
            }
            guard let self, self.signalAttentionCount == count else { return }
            try? await center.setBadgeCount(count)
        }
        #endif
    }

    var contextToken: Int { generation }
    var screenToken: Int { screen }
    func closeScreen(screen token: Int) { guard token == screen else { return }; closeScreen() }
    func editDraft(_ value: String, screen token: Int) { guard token == screen else { return }; draft = value }

    func editRawText(_ value: String, screen token: Int) {
        guard token == screen, controlEnabled, rawTyping else { return }; rawText = value
    }
    func setRawTyping(_ value: Bool, screen token: Int) {
        guard token == screen, controlEnabled else { return }; rawTyping = value
    }

    var agent: ControlledAgent? {
        guard let selected else { return nil }
        return machines.first { $0.id == selected.machine }?.agents.first { $0.id == selected.agent }
    }
    var destinationContext: AgentDestinationContext? {
        selected.map { AgentDestinationContext.resolve(scope: $0, machines: machines, connected: connected) }
    }
    var shortcuts: [AgentShortcut] {
        var seen = Set<String>()
        return machines.flatMap { machine in machine.agents.compactMap { agent -> AgentShortcut? in
            let scope = AgentScope(host: host, machine: machine.id, agent: agent.id)
            guard seen.insert(scope.storageKey).inserted else { return nil }
            return AgentShortcut(scope: scope, machine: machine, agent: agent)
        } }
    }
    var pinnedShortcuts: [AgentShortcut] {
        let entries = shortcuts.reduce(into: [String: AgentShortcut]()) { entries, shortcut in
            if entries[shortcut.id] == nil { entries[shortcut.id] = shortcut }
        }
        var seen = Set<String>()
        return navigation.pins.compactMap { key in
            guard seen.insert(key).inserted else { return nil }; return entries[key]
        }
    }
    var recentShortcuts: [AgentShortcut] {
        shortcuts.filter { !navigation.isPinned($0.scope) && !navigation.isHidden($0.scope) }.sorted {
            let left = navigation.recency($0.scope, startedAt: $0.agent.startedAt)
            let right = navigation.recency($1.scope, startedAt: $1.agent.startedAt)
            return left == right ? $0.id < $1.id : left > right
        }
    }
    func togglePin(_ scope: AgentScope) {
        if navigation.isPinned(scope) { navigation.unpin(scope) } else { navigation.pin(scope) }; saveNavigation()
    }
    func hideShortcut(_ scope: AgentScope) { navigation.hide(scope); saveNavigation() }
    private func recordInteraction(_ scope: AgentScope) { navigation.interact(scope); saveNavigation() }
    private func saveNavigation() {
        if let data = try? JSONEncoder().encode(navigation) { defaults.set(data, forKey: "pi.agentNavigation.v1") }
    }
    var controlEnabled: Bool { connected && permission == .control && streamId != nil && agent != nil && machines.contains { $0.id == selected?.machine && $0.online && $0.metadataStatus != "error" } }
    var sendExplanation: String? {
        if recoveryUnavailable {
            return "Saved prompt recovery is unavailable. Resolve this phone's storage before sending."
        }
        if recoveries.count >= 50 {
            return "Saved prompt recovery is full. Copy or recover and dismiss a saved prompt before sending."
        }
        if !connected { return "Connect to your computer to control agents." }
        if permission != .control { return "Terminal access is view only. Enable control on the computer you paired with." }
        if streamId == nil { return "Open the terminal before sending." }
        guard let agent else { return "Agent status is unavailable. Refresh before sending a prompt." }
        if let reason = agent.promptBlockReason { return reason }
        if composer.pending != nil {
            return "Waiting for a delivery result. This prompt will never resend automatically."
        }
        if composer.text.utf8.count > 65536 { return "Prompt exceeds 64 KiB. Shorten it before sending." }
        return nil
    }
    var canSend: Bool {
        sendExplanation == nil && !composer.text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
    }
    var draft: String {
        get { composer.text }
        set { composer.text = newValue; saveComposer() }
    }

    func setInventoryVisible(_ value: Bool) { guard inventoryVisible != value else { return }; inventoryVisible = value; if !value { inputRefreshTask?.cancel() }; restartVisibleInventory() }
    func setInventoryForeground(_ value: Bool) { guard inventoryForeground != value else { return }; inventoryForeground = value; updateSignalBadge(); if !value { inputRefreshTask?.cancel() }; restartVisibleInventory() }
    private func restartVisibleInventory() {
        visibleRefreshTask?.cancel(); visibleRefreshTask = nil
        guard inventoryVisible, inventoryForeground, connected, permission != .off else { return }
        let token = generation
        visibleRefreshTask = Task { [weak self] in
            while !Task.isCancelled {
                guard let self, token == self.generation, self.inventoryVisible, self.inventoryForeground, self.connected, self.permission != .off else { return }
                await self.refresh()
                try? await Task.sleep(nanoseconds: self.inventoryRefreshNanoseconds)
            }
        }
    }
    func revokeAccess() {
        workspaceManagement = false; workspaceSupported = false
        applyPermission(.off)
    }
    func transport(host: String, connected: Bool, request: Request? = nil) {
        visibleRefreshTask?.cancel(); visibleRefreshTask = nil
        generation += 1; inventoryInFlight = false; inputRefreshTask?.cancel(); inputRefreshTask = nil; refreshRevision += 1; refreshTask?.cancel(); refreshTask = nil
        // A pending send is uncertain after disconnect, never silently resent.
        if let submission = composer.pending {
            if let selected {
                settleRecovery(
                    submission.id, scope: selected, status: "Delivery uncertain",
                    reason: "Connection changed before confirmation. Inspect the terminal before sending again.")
            }
            composer.finish(
                submission, outcome: .failed,
                reason: "Connection changed. Delivery may be uncertain. Inspect the terminal before sending again.")
        }
        // Close through the originating transport before adopting another host.
        let resume = selected ?? resumeScope
        closeScreen()
        let sameHost = self.host == host
        resumeScope = sameHost ? resume : nil
        self.request = request; self.connected = connected
        self.host = host; if !sameHost { machines = []; permission = .off }; workspaceManagement = false; workspaceSupported = false; projectOrderSupported = false; permissionRevision += 1; terminalPermissionRevision += 1
        loading = false; error = nil; discovering = connected
        guard connected else { return }
        let token = generation
        refreshTask = Task { [weak self] in
            guard let self, let request else { return }
            let revision = self.permissionRevision
            do {
                let data = try await request("permissions", [:])
                guard token == self.generation else { return }
                if revision == self.permissionRevision {
                    self.applyWorkspacePermission(data)
                    self.applyPermission(try data["terminals"].decode(AgentTerminalPermission.self))
                }
                await self.refresh()
            } catch { if token == self.generation { self.error = error.localizedDescription } }
            if token == self.generation { self.discovering = false }
        }
    }

    func receive(_ envelope: WireEnvelope) {
        if envelope.type == "permissions" {
            workspaceManagement = envelope.workspaceManagement ?? false
            workspaceSupported = (try? envelope.workspace?["management"].decode(Bool.self)) ?? false
            projectOrderSupported = (try? envelope.workspace?["projectOrder"].decode(Bool.self)) ?? false
            applyPermission(AgentTerminalPermission(rawValue: envelope.terminals ?? "") ?? .off)
            return
        }
        guard permission != .off, connected, let id = envelope.streamId else { return }
        let event = AgentTerminalEvent(
            type: envelope.type, streamId: id, data: envelope.data?.text, cols: envelope.cols, rows: envelope.rows,
            reason: envelope.reason)
        if id == streamId { apply(event) } else if pendingOpen != nil { pendingOpen?.append(event) }
    }

    private func applyWorkspacePermission(_ data: JSONValue) {
        workspaceManagement = (try? data["workspaceManagement"].decode(Bool.self)) ?? false
        workspaceSupported = (try? data["workspace"]["management"].decode(Bool.self)) ?? false
        projectOrderSupported = (try? data["workspace"]["projectOrder"].decode(Bool.self)) ?? false
    }
    var canManage: Bool { connected && permission == .control && workspaceManagement && workspaceSupported }
    var bridgeIdentity: String { host }
    func catalog(machine: String, project: String) async throws -> WorkspaceCatalog {
        guard connected, permission != .off, let request else { throw AgentControlFailure(message: "Connect with terminal access to load launch options.") }
        let token = generation, revision = permissionRevision
        let value = try await request("workspace.catalog", ["machine": machine, "projectId": project]).decode(WorkspaceCatalog.self)
        guard token == generation, revision == permissionRevision, permission != .off else { throw AgentControlFailure(message: "Connection or read permission changed. Reload launch options.") }; return value
    }
    func workspaceRequest(_ operation: String, fields: [String: Any]) async throws -> WorkspaceOperation {
        guard canManage, let request else { throw AgentControlFailure(message: "Workspace management requires control and the separate workspace management grant in Connect Phone.") }
        let token = generation, revision = permissionRevision
        let value = try await request(operation, fields).decode(WorkspaceOperation.self)
        guard token == generation, revision == permissionRevision, canManage else { throw AgentControlFailure(message: "Connection or permission changed. Outcome uncertain; inspect Projects before launching again.") }
        await refresh()
        guard token == generation, revision == permissionRevision, canManage else { throw AgentControlFailure(message: "Connection or permission changed while refreshing. Operation may already have committed; check status.") }
        return value
    }
    var canReorderProjects: Bool { canManage && projectOrderSupported }
    /// Saves a machine's project order on that machine, so every device lists it the same way.
    func reorderProjects(machine: String, projectIds: [String]) async throws {
        guard canReorderProjects else { throw AgentControlFailure(message: "Reordering projects requires workspace management and an updated bridge.") }
        projectOrderRevision += 1
        projectOrdersPending += 1
        defer { projectOrdersPending -= 1 }
        applyProjectOrder(projectIds, machine: machine)
        let receipt = try await workspaceRequest(
            "workspace.reorderProjects",
            fields: ["operationId": UUID().uuidString, "machine": machine, "projectIds": projectIds])
        guard receipt.state == "succeeded" else {
            throw AgentControlFailure(message: receipt.message ?? "Project order wasn't saved. Refresh and try again.")
        }
        // The machine's saved order, in case a refresh that predates the move was skipped.
        if let saved = receipt.detail?.projectIds { applyProjectOrder(saved, machine: machine) }
    }
    private func applyProjectOrder(_ projectIds: [String], machine: String) {
        guard let index = machines.firstIndex(where: { $0.id == machine }),
              let projects = machines[index].registeredProjects else { return }
        machines[index].registeredProjects = Self.ordered(projects, by: projectIds)
    }
    /// `projects` in the order of `ids`; projects not listed keep their order after them.
    static func ordered(_ projects: [WorkspaceProject], by ids: [String]) -> [WorkspaceProject] {
        projects.enumerated().sorted { left, right in
            let l = ids.firstIndex(of: left.element.id) ?? Int.max, r = ids.firstIndex(of: right.element.id) ?? Int.max
            return l != r ? l < r : left.offset < right.offset
        }.map(\.element)
    }
    func refreshWorkspaceForReview(machine: String, project: String) async -> Bool {
        let token = generation, revision = permissionRevision
        guard canManage, await refresh(), token == generation, revision == permissionRevision, canManage,
              let current = machines.first(where: { $0.id == machine }), current.online, current.complete,
              current.metadataStatus == "complete", current.registeredProjects?.contains(where: { $0.id == project && $0.available }) == true else { return false }
        return true
    }
    func operationStatus(_ id: String) async throws -> WorkspaceOperation {
        guard connected, permission != .off, let request else { throw AgentControlFailure(message: "Reconnect with terminal access to check status.") }
        let token = generation, revision = permissionRevision
        let value = try await request("workspace.operation", ["operationId": id]).decode(WorkspaceOperation.self)
        guard token == generation, revision == permissionRevision, permission != .off else { throw AgentControlFailure(message: "Connection or read permission changed. Check status again.") }; return value
    }
    private var inventoryCacheKey: String { "pi.workspaceInventory.v1." + Data(host.utf8).base64EncodedString() }

    private func applyPermission(_ value: AgentTerminalPermission) {
        permissionRevision += 1
        let previous = permission
        if previous != value { terminalPermissionRevision += 1 }
        permission = value
        restartVisibleInventory()
        if value != .control { rawTyping = false; rawText = "" }
        if value == .off {
            machines = []; defaults.removeObject(forKey: inventoryCacheKey); refreshRevision += 1; closeScreen()
        } else if previous != value, pendingOpen != nil {
            // An open begun under a different permission is never adopted.
            closeScreen()
        }
        if previous == .off && value != .off && machines.isEmpty,
           let data = defaults.data(forKey: inventoryCacheKey), let cached = try? JSONDecoder().decode(AgentInventory.self, from: data) {
            machines = cached.normalized.machines.map { machine in
                AgentMachine(id: machine.id, name: machine.name, online: false, protocolVersion: machine.protocolVersion, screenMayBeInexact: machine.screenMayBeInexact, error: "Cached context; refreshing availability", complete: false, agents: machine.agents, registeredProjects: machine.registeredProjects, worktrees: machine.worktrees, metadataStatus: "offline", metadataError: "Cached context; refresh required")
            }
        }
        if previous == .off && value != .off && !discovering { Task { [weak self] in await self?.refresh() } }
    }

    @discardableResult func refresh() async -> Bool {
        guard connected, permission != .off, let request else { return false }
        guard !inventoryInFlight else { return false }
        inventoryInFlight = true
        let owningGeneration = generation
        defer { if generation == owningGeneration { inventoryInFlight = false } }
        refreshRevision += 1; let revision = refreshRevision; let token = generation
        let orderRevision = projectOrderRevision
        loading = true
        do {
            let data = try await request("agents", [:])
            let inventory = try data.decode(AgentInventory.self).normalized
            guard token == generation, revision == refreshRevision, permission != .off else { return false }
            let prior = machines
            machines = inventory.machines.map { machine in
                guard (!machine.online || machine.metadataStatus == "error"), let cached = prior.first(where: { $0.id == machine.id }) else { return machine }
                return AgentMachine(id: machine.id, name: machine.name, online: machine.online, protocolVersion: machine.protocolVersion, screenMayBeInexact: machine.screenMayBeInexact, error: machine.error, complete: false, agents: cached.agents, registeredProjects: cached.registeredProjects, worktrees: cached.worktrees, metadataStatus: machine.metadataStatus ?? "offline", metadataError: machine.metadataError ?? machine.error)
            }.map { machine in
                // Fetched across a reorder: its project order may predate it, so keep the one on screen.
                guard orderRevision != projectOrderRevision || projectOrdersPending > 0,
                      let shown = prior.first(where: { $0.id == machine.id })?.registeredProjects?.map(\.id),
                      let projects = machine.registeredProjects else { return machine }
                var kept = machine
                kept.registeredProjects = Self.ordered(projects, by: shown)
                return kept
            }.sorted {
                let left = $0.agents.contains { $0.phase == "dialog" },
                    right = $1.agents.contains { $0.phase == "dialog" }
                return left != right ? left : $0.name.localizedStandardCompare($1.name) == .orderedAscending
            }
            if let data = try? JSONEncoder().encode(AgentInventory(machines: machines)) { defaults.set(data, forKey: inventoryCacheKey) }
            loading = false
            resumeAfterReconnect()
            return true
        } catch {
            guard token == generation, revision == refreshRevision else { return false }
            loading = false; self.error = error.localizedDescription
            machines = machines.map { machine in
                AgentMachine(id: machine.id, name: machine.name, online: false, protocolVersion: machine.protocolVersion, screenMayBeInexact: machine.screenMayBeInexact, error: "Cached inventory unavailable. Refresh to retry.", complete: false, agents: machine.agents, registeredProjects: machine.registeredProjects, worktrees: machine.worktrees, metadataStatus: "error", metadataError: "Inventory refresh failed")
            }
            return false
        }
    }

    func open(machine: AgentMachine, agent: ControlledAgent, context token: Int, interactive: Bool = true) {
        guard token == generation, connected, permission != .off, machine.online, machine.metadataStatus != "error", let request else { return }
        closeScreen()
        let scope = AgentScope(host: host, machine: machine.id, agent: agent.id)
        // A lifecycle reopen is not a navigation choice, so it never touches Recents or hidden entries.
        if interactive { recordInteraction(scope) }
        selected = scope; composer = composers[scope] ?? AgentComposer(text: drafts[scope.storageKey] ?? "")
        recoveries = savedRecoveries[scope.storageKey] ?? []
        screenMayBeInexact = machine.screenMayBeInexact
        let token = generation, visible = screen
        pendingOpen = AgentPendingOpen(); error = nil
        Task { [weak self] in
            do {
                guard let self, token == self.generation, visible == self.screen, self.permission != .off else {
                    return
                }
                let result = try await request("terminal.open", ["machine": scope.machine, "agentId": scope.agent])
                    .decode(AgentTerminalOpen.self)
                guard token == self.generation, visible == self.screen, self.selected == scope, self.permission != .off
                else {
                    _ = try? await request("terminal.close", ["streamId": result.streamId]); return
                }
                guard result.machine == scope.machine, result.agentId == scope.agent,
                    self.pendingOpen?.overflowed == false
                else {
                    self.pendingOpen = nil
                    _ = try? await request("terminal.close", ["streamId": result.streamId])
                    guard token == self.generation, visible == self.screen else { return }
                    self.error = "Terminal opening could not retain a complete snapshot. Close and reopen the agent."
                    return
                }
                let events = self.pendingOpen?.events ?? []; self.pendingOpen = nil
                self.streamId = result.streamId; self.screenMayBeInexact = result.screenMayBeInexact
                for event in events where event.streamId == result.streamId { self.apply(event) }
            } catch {
                guard let self, token == self.generation, visible == self.screen else { return }
                self.pendingOpen = nil; self.error = error.localizedDescription
            }
        }
    }

    private func resumeAfterReconnect() {
        guard let scope = resumeScope else { return }
        resumeScope = nil
        guard selected == nil, scope.host == host,
            let machine = machines.first(where: { $0.id == scope.machine && $0.online && $0.metadataStatus != "error" }),
            let agent = machine.agents.first(where: { $0.id == scope.agent })
        else { return }
        open(machine: machine, agent: agent, context: generation, interactive: false)
    }

    func closeScreen() {
        resumeScope = nil
        if let submission = composer.pending {
            if let selected {
                settleRecovery(
                    submission.id, scope: selected, status: "Delivery uncertain",
                    reason: "Agent screen closed before confirmation. Inspect the terminal before sending again.")
            }
            composer.finish(
                submission, outcome: .failed,
                reason: "Agent screen closed before confirmation. Inspect the terminal before sending again.")
        }
        saveComposer(); screen += 1; pendingOpen = nil
        if let id = streamId, let request { Task { _ = try? await request("terminal.close", ["streamId": id]) } }
        selected = nil; streamId = nil; claimedSize = nil; rawTyping = false; rawText = ""; composer = AgentComposer(); recoveries = []
        attachUpload = nil; attachState = .idle
        cols = 80; rows = 24; hasSize = false; resetTerminal()
    }

    private func saveComposer() {
        guard let selected else { return }
        composers[selected] = composer
        drafts[selected.storageKey] = composer.text
        defaults.set(drafts, forKey: "pi.agentDrafts.v1")
    }

    private func persistRecovery() {
        guard !recoveryUnavailable else { return }
        do { defaults.set(try JSONEncoder().encode(savedRecoveries), forKey: "pi.agentRecovery.v1") } catch {
            recoveryUnavailable = true;
            self.error = "Could not save prompt recovery. Text remains on this phone; resolve storage before sending."
        }
    }
    private func settleRecovery(_ id: UUID, scope: AgentScope, status: String?, reason: String?) {
        guard var items = savedRecoveries[scope.storageKey], let index = items.firstIndex(where: { $0.id == id }) else {
            return
        }
        if let status { items[index].status = status; items[index].reason = reason } else { items.remove(at: index) }
        savedRecoveries[scope.storageKey] = items
        if selected == scope { recoveries = items }
        persistRecovery()
    }
    func restoreRecovery(_ id: UUID, screen token: Int) {
        guard token == screen, let item = recoveries.first(where: { $0.id == id }) else { return }
        if draft.isEmpty { draft = item.text } else if draft != item.text { draft += "\n\n" + item.text }
    }
    func dismissRecovery(_ id: UUID, screen token: Int) {
        guard token == screen, composer.pending?.id != id, let selected else { return }
        settleRecovery(id, scope: selected, status: nil, reason: nil)
    }

    func sendPrompt(screen token: Int) {
        guard token == screen, canSend, let scope = selected, let request, let submission = composer.begin() else {
            return
        }
        savedRecoveries[scope.storageKey, default: []].append(
            AgentPromptRecovery(
                id: submission.id, text: submission.text, status: "Delivery uncertain",
                reason: "Awaiting confirmation. This prompt will never resend automatically."))
        recoveries = savedRecoveries[scope.storageKey] ?? []
        persistRecovery(); saveComposer()
        guard !recoveryUnavailable else {
            composer.finish(
                submission, outcome: .failed, reason: "Prompt recovery could not be saved. Text kept on this phone.")
            saveComposer(); return
        }
        recordInteraction(scope)
        let token = generation, visible = screen, permissionToken = terminalPermissionRevision
        Task { [weak self] in
            let result: AgentPromptResult
            var deliveryUncertain = false
            do {
                guard let self, token == self.generation, visible == self.screen,
                    permissionToken == self.terminalPermissionRevision, self.permission == .control
                else {
                    throw AgentControlFailure(
                        message: "Agent selection or permission changed before sending. Text kept on this phone.")
                }
                result = try await request(
                    "agent.prompt", ["machine": scope.machine, "agentId": scope.agent, "text": submission.text]
                ).decode(AgentPromptResult.self)
            } catch { deliveryUncertain = true; result = AgentPromptResult(outcome: .failed, reason: "Delivery uncertain: " + error.localizedDescription) }
            guard let self else { return }
            let valid = token == self.generation && permissionToken == self.terminalPermissionRevision
            let outcome =
                valid
                ? result
                : AgentPromptResult(
                    outcome: .failed,
                    reason:
                        "Connection or permission changed. Delivery may be uncertain. Inspect the terminal before sending again."
                )
            var origin = self.composers[scope] ?? AgentComposer(text: submission.text)
            if visible == self.screen && self.selected == scope { origin = self.composer }
            if valid && origin.pending?.id == submission.id
                && (outcome.outcome == .confirmed || outcome.outcome == .sent)
            {
                self.settleRecovery(submission.id, scope: scope, status: nil, reason: nil)
            } else if outcome.outcome == .failed || outcome.outcome == .blocked {
                self.settleRecovery(
                    submission.id, scope: scope, status: valid && !deliveryUncertain ? outcome.outcome.label : "Delivery uncertain",
                    reason: outcome.reason)
            }
            origin.finish(submission, outcome: outcome.outcome, reason: outcome.reason, uncertain: deliveryUncertain || !valid)
            self.composers[scope] = origin; self.drafts[scope.storageKey] = origin.text
            self.defaults.set(self.drafts, forKey: "pi.agentDrafts.v1")
            if visible == self.screen && self.selected == scope {
                self.composer = origin
                if valid { await self.refresh() }
            }
        }
    }

    /// Uploads files to the open agent's machine in order, then adds their paths to that agent's
    /// draft without sending it, so the user writes the prompt around them.
    func attach(_ files: [AgentAttachment], screen token: Int) {
        guard token == screen, controlEnabled, !files.isEmpty, let scope = selected, let request else { return }
        if case .uploading = attachState { return }
        if let large = files.first(where: { $0.data.count > AgentAttachment.maxBytes }) {
            attachState = .failed("\(large.name) is larger than 20 MB.")
            return
        }
        attachState = .uploading(files.count == 1 ? "Attaching \(files[0].name)…" : "Attaching \(files.count) files…")
        let generationToken = generation, permissionToken = terminalPermissionRevision, upload = UUID()
        attachUpload = upload
        Task { [weak self] in
            var paths: [String] = []
            var failure: String?
            do {
                for file in files {
                    let uploadId = UUID().uuidString.lowercased()
                    var path: String?
                    for chunk in file.chunks {
                        guard let self, generationToken == self.generation,
                            permissionToken == self.terminalPermissionRevision, self.permission == .control
                        else { throw AgentControlFailure(message: "Connection or permission changed while attaching. Attach again.") }
                        let reply = try await request(
                            "agent.attach",
                            [
                                "machine": scope.machine, "agentId": scope.agent, "uploadId": uploadId, "name": file.name,
                                "offset": chunk.offset, "data": chunk.data.base64EncodedString(), "done": chunk.done,
                            ]
                        ).decode(AgentAttachReply.self)
                        path = reply.path
                    }
                    guard let path else { throw AgentControlFailure(message: "The agent's machine did not accept \(file.name).") }
                    paths.append(path)
                }
            } catch {
                failure = error.localizedDescription
            }
            guard let self else { return }
            // Files that did upload are on the agent's machine, so their paths still go in the draft.
            if !paths.isEmpty {
                if self.selected == scope {
                    self.draft = AgentAttachment.draft(self.draft, adding: paths)
                } else {
                    var origin = self.composers[scope] ?? AgentComposer(text: self.drafts[scope.storageKey] ?? "")
                    origin.text = AgentAttachment.draft(origin.text, adding: paths)
                    self.composers[scope] = origin; self.drafts[scope.storageKey] = origin.text
                    self.defaults.set(self.drafts, forKey: "pi.agentDrafts.v1")
                }
            }
            guard self.attachUpload == upload else { return }
            self.attachUpload = nil
            self.attachState = failure.map { .failed($0) } ?? .idle
        }
    }
    func dismissAttachError() { if case .failed = attachState { attachState = .idle } }

    func sendKey(_ key: AgentQuickKey, screen token: Int) {
        guard token == screen else { return }; input("terminal.keys", fields: ["key": key.rawValue])
    }
    // Called only while the terminal synthesizes a deliberate mouse-wheel event.
    func sendScroll(_ data: ArraySlice<UInt8>, screen token: Int) {
        guard token == screen, controlEnabled, !data.isEmpty else { return }
        input("terminal.raw", fields: ["data": Data(data).base64EncodedString(), "encoding": "base64"])
    }
    func sendRaw(screen token: Int) {
        guard token == screen, rawTyping, !rawText.isEmpty, rawText.utf8.count <= 65536 else { return }
        input("terminal.raw", fields: ["data": rawText])
    }
    /// Ask the agent's PTY to take the phone's geometry while this screen views it (Control only).
    /// Closing the stream releases the claim and the daemon restores the previous owner's size.
    func claimSize(cols: Int, rows: Int, screen token: Int) {
        guard token == screen, controlEnabled, !screenMayBeInexact, let streamId, let request else { return }
        let cols = min(max(cols, 20), 500), rows = min(max(rows, 8), 300)
        if let claimed = claimedSize, claimed.cols == cols, claimed.rows == rows, claimed.stream == streamId { return }
        claimedSize = (cols, rows, streamId)
        let generationToken = generation, visible = screen
        Task { [weak self] in
            do {
                _ = try await request("terminal.size", ["streamId": streamId, "cols": cols, "rows": rows])
            } catch {
                guard let self, generationToken == self.generation, visible == self.screen else { return }
                if self.claimedSize?.stream == streamId { self.claimedSize = nil }
                // A daemon without phone sizing keeps the Mac's geometry; zoom still works.
                if !error.localizedDescription.contains("does not support") { self.error = error.localizedDescription }
            }
        }
    }
    private func scheduleInputRefresh() {
        guard inventoryVisible, inventoryForeground else { return }
        inputRefreshTask?.cancel()
        let token = generation
        inputRefreshTask = Task { [weak self] in
            try? await Task.sleep(nanoseconds: 250_000_000)
            guard !Task.isCancelled, let self, token == self.generation else { return }
            await self.refresh()
        }
    }
    private func input(_ op: String, fields: [String: Any]) {
        guard controlEnabled, let streamId, let request else { return }
        let token = generation, visible = screen, permissionToken = terminalPermissionRevision
        var fields = fields; fields["streamId"] = streamId
        Task { [weak self] in
            do {
                guard let self, token == self.generation, visible == self.screen,
                    permissionToken == self.terminalPermissionRevision, self.controlEnabled
                else { return }
                _ = try await request(op, fields)
                guard token == self.generation, visible == self.screen, permissionToken == self.terminalPermissionRevision
                else { return }
                if op == "terminal.raw", self.rawText == fields["data"] as? String { self.rawText = "" }
                self.scheduleInputRefresh()
            } catch {
                guard let self, token == self.generation, visible == self.screen else { return }
                self.error = error.localizedDescription
            }
        }
    }

    private func resetTerminal() {
        terminalChunks = []; terminalBytes = 0; terminalBaseCols = cols; terminalBaseRows = rows
        terminalEpoch = UUID(); terminalRevision += 1
    }
    private func apply(_ event: AgentTerminalEvent) {
        switch event.type {
        case "terminal.output":
            guard let data = event.data else { return }
            // Never discard part of a terminal stream then continue with corrupt state.
            // Close this viewer if SwiftUI cannot drain it; the agent keeps running.
            guard terminalBytes + data.utf8.count <= 1024 * 1024 else {
                self.error = "Terminal display fell behind. Close and reopen the agent for a fresh snapshot."
                if let streamId, let request {
                    Task { _ = try? await request("terminal.close", ["streamId": streamId]) }
                }
                streamId = nil; resetTerminal(); return
            }
            terminalRevision += 1; terminalChunks.append(.output(data)); terminalBytes += data.utf8.count
        case "terminal.size":
            guard let cols = event.cols, let rows = event.rows, (1...2000).contains(cols), (1...1000).contains(rows)
            else { return }
            terminalChunks.append(.size(cols, rows)); terminalRevision += 1
            self.cols = cols; self.rows = rows; hasSize = true
        case "terminal.reset": resetTerminal()
        case "terminal.closed":
            streamId = nil; rawTyping = false; rawText = ""; error = event.reason ?? "Terminal closed."
        default: break
        }
    }
    func drainTerminal() -> [AgentTerminalUpdate] {
        let chunks = terminalChunks; terminalChunks = []; terminalBytes = 0; return chunks
    }
}
