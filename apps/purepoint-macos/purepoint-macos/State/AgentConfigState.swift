import Foundation
import Observation

@Observable
@MainActor
final class AgentConfigState {
    var agents: [AgentConfigPayload] = []
    var defaultAgent: String = "claude"
    var isLoading = false
    var error: String?
    var agentStatusTracking = true
    /// Machine-level ("all projects") launch args string per agent type.
    var machineLaunchArgs: [String: String] = [:]
    /// Machine-level ("all projects") command per agent type.
    var machineCommands: [String: String] = [:]
    var globalSettingsLoaded = false
    var isSavingGlobalSettings = false
    var globalError: String?

    @ObservationIgnored private let ensureDaemon: () async throws -> Void
    @ObservationIgnored private let sendRequest: (DaemonRequest) async throws -> DaemonResponse

    #if os(macOS)
    init(
        ensureDaemon: @escaping () async throws -> Void = { try await DaemonLifecycle.ensureDaemon() },
        sendRequest: ((DaemonRequest) async throws -> DaemonResponse)? = nil
    ) {
        self.ensureDaemon = ensureDaemon
        let client = DaemonEndpoint.local.makeClient()
        self.sendRequest = sendRequest ?? { try await client.send($0) }
    }
    #else
    /// iOS has no local daemon to start. `AppState` builds one per machine on that machine's
    /// client (IPAD-012); these defaults report that no daemon is running.
    init(
        ensureDaemon: @escaping () async throws -> Void = { throw DaemonClientError.notRunning },
        sendRequest: ((DaemonRequest) async throws -> DaemonResponse)? = nil
    ) {
        self.ensureDaemon = ensureDaemon
        self.sendRequest = sendRequest ?? { _ in throw DaemonClientError.notRunning }
    }
    #endif

    func loadGlobalSettings() async {
        await sendGlobalSettings(.getGlobalAgentSettings)
    }

    /// The agents in the All Projects list: built-ins, then the ones the user added.
    var machineAgentNames: [String] {
        let added = Set(machineCommands.keys).union(machineLaunchArgs.keys).subtracting(AgentLaunchDefaults.builtIns)
        return AgentLaunchDefaults.builtIns + added.sorted()
    }

    /// Save the machine-level launch args for one agent type; a blank string removes them.
    func updateMachineLaunchArgs(agentName: String, launchArgs: String, projectRoot: String?) async {
        await saveMachineSettings(.updateGlobalAgentSettings(launchArgs: [agentName: launchArgs]), projectRoot: projectRoot)
    }

    /// Save one agent's machine-level command and launch args; blank strings remove them.
    func updateMachineAgent(agentName: String, command: String, launchArgs: String, projectRoot: String?) async {
        await saveMachineSettings(
            .updateGlobalAgentSettings(launchArgs: [agentName: launchArgs], commands: [agentName: command]),
            projectRoot: projectRoot)
    }

    /// Add an agent for every project; its type is named after the command.
    func addMachineAgent(command: String, launchArgs: String, projectRoot: String?) async {
        let name = AgentLaunchDefaults.agentName(for: command, taken: Set(machineAgentNames + agents.map(\.agentType)))
        await updateMachineAgent(agentName: name, command: command, launchArgs: launchArgs, projectRoot: projectRoot)
    }

    func removeMachineAgent(agentName: String, projectRoot: String?) async {
        await saveMachineSettings(.updateGlobalAgentSettings(removeAgents: [agentName]), projectRoot: projectRoot)
    }

    private func saveMachineSettings(_ request: DaemonRequest, projectRoot: String?) async {
        guard !isSavingGlobalSettings else { return }
        isSavingGlobalSettings = true
        await sendGlobalSettings(request)
        if let projectRoot { await load(projectRoot: projectRoot) }
        isSavingGlobalSettings = false
    }

    func updateAgentStatusTracking(_ enabled: Bool) async {
        guard !isSavingGlobalSettings else { return }
        isSavingGlobalSettings = true
        await sendGlobalSettings(.updateGlobalAgentSettings(agentStatusTracking: enabled))
        isSavingGlobalSettings = false
    }

    private func sendGlobalSettings(_ request: DaemonRequest) async {
        globalError = nil
        do {
            try await ensureDaemon()
            switch try await sendRequest(request) {
            case .globalAgentSettingsReport(let tracking, let launchArgs, let commands):
                agentStatusTracking = tracking
                globalSettingsLoaded = true
                if let launchArgs {
                    machineLaunchArgs = launchArgs
                } else if case .updateGlobalAgentSettings(_, .some, _, _) = request {
                    globalError = Self.oldDaemonMessage("save launch args for all projects")
                }
                if let commands {
                    machineCommands = commands
                    publishAddedAgents()
                } else if case .updateGlobalAgentSettings(_, _, let changed, let removed) = request,
                    changed != nil || removed != nil
                {
                    globalError = Self.oldDaemonMessage("edit agent commands for all projects")
                }
            case .error(_, let message):
                globalError = message
            default:
                globalError = "Unexpected response. Update the PurePoint daemon to use global settings."
            }
        } catch {
            globalError = error.localizedDescription
        }
    }

    func load(projectRoot: String) async {
        isLoading = true
        error = nil
        do {
            let response = try await sendRequest(.getConfig(projectRoot: projectRoot))
            switch response {
            case .configReport(let defaultAgent, let agents):
                applyConfigReport(defaultAgent: defaultAgent, agents: agents)
            case .error(_, let message):
                self.error = message
            default:
                self.error = "Unexpected response"
            }
        } catch {
            self.error = error.localizedDescription
        }
        isLoading = false
    }

    /// Save the project's launch args string; `nil` clears it so the machine default applies.
    func updateLaunchArgs(projectRoot: String, agentName: String, launchArgs: String?) async {
        await sendProjectUpdate(.updateAgentConfig(projectRoot: projectRoot, agentName: agentName, launchArgs: launchArgs))
    }

    /// Save one agent's project command and launch args; a blank value clears it so the
    /// All Projects value applies.
    func updateProjectAgent(projectRoot: String, agentName: String, command: String, launchArgs: String) async {
        let args = launchArgs.trimmingCharacters(in: .whitespaces)
        await sendProjectUpdate(
            .updateAgentConfig(
                projectRoot: projectRoot, agentName: agentName, launchArgs: args.isEmpty ? nil : args,
                command: command))
    }

    /// Add an agent to this project only; its type is named after the command.
    func addProjectAgent(projectRoot: String, command: String, launchArgs: String) async {
        let name = AgentLaunchDefaults.agentName(for: command, taken: Set(machineAgentNames + agents.map(\.agentType)))
        await updateProjectAgent(projectRoot: projectRoot, agentName: name, command: command, launchArgs: launchArgs)
    }

    func removeProjectAgent(projectRoot: String, agentName: String) async {
        await sendProjectUpdate(
            .updateAgentConfig(projectRoot: projectRoot, agentName: agentName, launchArgs: nil, remove: true))
    }

    private func sendProjectUpdate(_ request: DaemonRequest) async {
        error = nil
        do {
            let response = try await sendRequest(request)
            switch response {
            case .configReport(let defaultAgent, let agents):
                applyConfigReport(defaultAgent: defaultAgent, agents: agents)
            case .error(_, let message):
                self.error = message
            default:
                self.error = "Unexpected response"
            }
        } catch {
            self.error = error.localizedDescription
        }
    }

    private func applyConfigReport(defaultAgent: String, agents: [AgentConfigPayload]) {
        self.defaultAgent = defaultAgent
        self.agents = agents
        publishAddedAgents()
        // Daemons before editable commands omit `inherited_command`.
        if agents.contains(where: { $0.inheritedCommand == nil }) {
            self.error = Self.oldDaemonMessage("edit agent commands")
        }
    }

    /// Offer the agents the user added wherever a new agent can be started.
    private func publishAddedAgents() {
        var commands: [String: String] = [:]
        for agent in agents where !AgentLaunchDefaults.builtIns.contains(agent.agentType) {
            commands[agent.agentType] = agent.command
        }
        for name in machineAgentNames where !AgentLaunchDefaults.builtIns.contains(name) && commands[name] == nil {
            commands[name] = machineCommands[name] ?? name
        }
        AgentVariant.addedAgents = commands.keys.sorted().map { AgentVariant.added(name: $0, command: commands[$0] ?? $0) }
    }

    private static func oldDaemonMessage(_ action: String) -> String {
        "Update the PurePoint daemon to \(action). Restart it after updating."
    }

    func agentConfig(named name: String) -> AgentConfigPayload? {
        agents.first { $0.agentType == name }
    }
}

/// What PurePoint knows about agent commands before the user changes anything.
enum AgentLaunchDefaults {
    /// Agent types PurePoint ships, in the order Settings lists them.
    static let builtIns = ["claude", "codex", "opencode"]

    /// Words that start another program rather than name the agent.
    private static let runners: Set<String> = [
        "env", "exec", "npx", "pnpx", "bunx", "uvx", "pipx", "node", "bun", "deno", "python", "python3",
    ]

    /// An agent type for a new agent: the program the command runs (`npx aider` → `aider`,
    /// `scripts/agent-launch.sh goose` → `goose`), made unique among `taken`.
    static func agentName(for command: String, taken: Set<String>) -> String {
        let words = command.split(whereSeparator: \.isWhitespace).map { (String($0) as NSString).lastPathComponent }
        let program = words.first { !$0.hasPrefix("-") && !runners.contains($0) && !$0.hasSuffix(".sh") }
            ?? words.first ?? ""
        let stem = ((program as NSString).deletingPathExtension).lowercased()
        var slug = String(stem.map { $0.isASCII && ($0.isLetter || $0.isNumber || $0 == "-" || $0 == "_") ? $0 : "-" })
        while let first = slug.first, first == "-" || first == "_" { slug.removeFirst() }
        slug = String(slug.prefix(60))
        if slug.isEmpty { slug = "agent" }
        guard taken.contains(slug) else { return slug }
        var n = 2
        while taken.contains("\(slug)-\(n)") { n += 1 }
        return "\(slug)-\(n)"
    }
}
