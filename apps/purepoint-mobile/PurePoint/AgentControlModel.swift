import Foundation
import SwiftUI

@MainActor final class AgentControlModel: ObservableObject {
    typealias Request = @MainActor (String, [String: Any]) async throws -> JSONValue
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
    @Published var error: String?
    @Published var rawTyping = false
    @Published var rawText = ""
    private var host = ""
    private var request: Request?
    private var generation = 0
    private var screen = 0
    private var permissionRevision = 0
    private var refreshRevision = 0
    private var pendingOpen: AgentPendingOpen?
    private var refreshTask: Task<Void, Never>?
    private var discovering = false
    private var terminalChunks: [AgentTerminalUpdate] = []
    private(set) var terminalBaseCols = 80
    private(set) var terminalBaseRows = 24
    private var terminalBytes = 0
    private var drafts: [String: String]
    private var composers: [AgentScope: AgentComposer] = [:]
    private let defaults: UserDefaults

    init(defaults: UserDefaults = .standard) {
        self.defaults = defaults
        drafts = defaults.dictionary(forKey: "pi.agentDrafts.v1") as? [String: String] ?? [:]
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
    var controlEnabled: Bool { connected && permission == .control && selected != nil && streamId != nil }
    var sendExplanation: String? {
        if !connected { return "Connect to your Mac to control agents." }
        if permission != .control { return "Terminal access is view only. Enable control on your Mac." }
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

    func transport(host: String, connected: Bool, request: Request? = nil) {
        generation += 1; refreshRevision += 1; refreshTask?.cancel(); refreshTask = nil
        // A pending send is uncertain after disconnect, never silently resent.
        if let submission = composer.pending {
            composer.finish(
                submission, outcome: .failed,
                reason: "Connection changed. Delivery may be uncertain. Inspect the terminal before sending again.")
        }
        // Close through the originating transport before adopting another host.
        closeScreen()
        self.request = request; self.connected = connected
        self.host = host; machines = []; permission = .off; permissionRevision += 1
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
                    self.applyPermission(try data["terminals"].decode(AgentTerminalPermission.self))
                }
                await self.refresh()
            } catch { if token == self.generation { self.error = error.localizedDescription } }
            if token == self.generation { self.discovering = false }
        }
    }

    func waitForDiscovery() async { await refreshTask?.value }

    func receive(_ envelope: WireEnvelope) {
        if envelope.type == "permissions" {
            applyPermission(AgentTerminalPermission(rawValue: envelope.terminals ?? "") ?? .off)
            return
        }
        guard permission != .off, connected, let id = envelope.streamId else { return }
        let event = AgentTerminalEvent(
            type: envelope.type, streamId: id, data: envelope.data?.text, cols: envelope.cols, rows: envelope.rows,
            reason: envelope.reason)
        if id == streamId { apply(event) } else if pendingOpen != nil { pendingOpen?.append(event) }
    }

    private func applyPermission(_ value: AgentTerminalPermission) {
        permissionRevision += 1
        let previous = permission
        permission = value
        if value != .control { rawTyping = false; rawText = "" }
        if value == .off {
            machines = []; refreshRevision += 1; closeScreen()
        } else if previous != value, pendingOpen != nil {
            // An open begun under a different permission is never adopted.
            closeScreen()
        }
        if previous == .off && value != .off && !discovering { Task { [weak self] in await self?.refresh() } }
    }

    func refresh() async {
        guard connected, permission != .off, let request else { return }
        refreshRevision += 1; let revision = refreshRevision; let token = generation
        loading = true
        do {
            let data = try await request("agents", [:])
            let inventory = try data.decode(AgentInventory.self)
            guard token == generation, revision == refreshRevision, permission != .off else { return }
            machines = inventory.machines.sorted {
                let left = $0.agents.contains { $0.phase == "dialog" },
                    right = $1.agents.contains { $0.phase == "dialog" }
                return left != right ? left : $0.name.localizedStandardCompare($1.name) == .orderedAscending
            }
            loading = false
        } catch {
            guard token == generation, revision == refreshRevision else { return }
            loading = false; self.error = error.localizedDescription
        }
    }

    func open(machine: AgentMachine, agent: ControlledAgent, context token: Int) {
        guard token == generation, connected, permission != .off, machine.online, let request else { return }
        closeScreen()
        let scope = AgentScope(host: host, machine: machine.id, agent: agent.id)
        selected = scope; composer = composers[scope] ?? AgentComposer(text: drafts[scope.storageKey] ?? "")
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

    func closeScreen() {
        if let submission = composer.pending {
            composer.finish(
                submission, outcome: .failed,
                reason: "Agent screen closed before confirmation. Inspect the terminal before sending again.")
        }
        saveComposer(); screen += 1; pendingOpen = nil
        if let id = streamId, let request { Task { _ = try? await request("terminal.close", ["streamId": id]) } }
        selected = nil; streamId = nil; rawTyping = false; rawText = ""; composer = AgentComposer()
        cols = 80; rows = 24; hasSize = false; resetTerminal()
    }

    private func saveComposer() {
        guard let selected else { return }
        composers[selected] = composer
        drafts[selected.storageKey] = composer.text
        defaults.set(drafts, forKey: "pi.agentDrafts.v1")
    }

    func sendPrompt(screen token: Int) {
        guard token == screen, canSend, let scope = selected, let request, let submission = composer.begin() else {
            return
        }
        saveComposer()
        let token = generation, visible = screen, permissionToken = permissionRevision
        Task { [weak self] in
            let result: AgentPromptResult
            do {
                guard let self, token == self.generation, visible == self.screen,
                    permissionToken == self.permissionRevision, self.permission == .control
                else {
                    throw AgentControlFailure(
                        message: "Agent selection or permission changed before sending. Text kept on this phone.")
                }
                result = try await request(
                    "agent.prompt", ["machine": scope.machine, "agentId": scope.agent, "text": submission.text]
                ).decode(AgentPromptResult.self)
            } catch { result = AgentPromptResult(outcome: .failed, reason: error.localizedDescription) }
            guard let self else { return }
            let valid = token == self.generation && permissionToken == self.permissionRevision
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
            origin.finish(submission, outcome: outcome.outcome, reason: outcome.reason)
            self.composers[scope] = origin; self.drafts[scope.storageKey] = origin.text
            self.defaults.set(self.drafts, forKey: "pi.agentDrafts.v1")
            if visible == self.screen && self.selected == scope {
                self.composer = origin
                if valid { await self.refresh() }
            }
        }
    }

    func sendKey(_ key: AgentQuickKey, screen token: Int) {
        guard token == screen else { return }; input("terminal.keys", fields: ["key": key.rawValue])
    }
    func sendRaw(screen token: Int) {
        guard token == screen, rawTyping, !rawText.isEmpty, rawText.utf8.count <= 65536 else { return }
        input("terminal.raw", fields: ["data": rawText])
    }
    private func input(_ op: String, fields: [String: Any]) {
        guard controlEnabled, let streamId, let request else { return }
        let token = generation, visible = screen, permissionToken = permissionRevision
        var fields = fields; fields["streamId"] = streamId
        Task { [weak self] in
            do {
                guard let self, token == self.generation, visible == self.screen,
                    permissionToken == self.permissionRevision, self.controlEnabled
                else { return }
                _ = try await request(op, fields)
                guard token == self.generation, visible == self.screen, permissionToken == self.permissionRevision
                else { return }
                if op == "terminal.raw", self.rawText == fields["data"] as? String { self.rawText = "" }
                await self.refresh()
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
                error = "Terminal display fell behind. Close and reopen the agent for a fresh snapshot."
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
