import Foundation
import Observation

@Observable
@MainActor
final class AgentsHubState {
    var prompts: [SavedPrompt] = []
    var agents: [AgentDefinition] = []
    var swarms: [SwarmDefinition] = []

    var selectedPromptId: String?
    var selectedAgentId: String?
    var selectedSwarmId: String?

    var showingCreatePrompt = false
    var showingCreateAgent = false
    var showingCreateSwarm = false

    var isLoading = false
    var error: String?

    var selectedPrompt: SavedPrompt? {
        prompts.first { $0.id == selectedPromptId }
    }
    var selectedAgent: AgentDefinition? {
        agents.first { $0.id == selectedAgentId }
    }
    var selectedSwarm: SwarmDefinition? {
        swarms.first { $0.id == selectedSwarmId }
    }

    @ObservationIgnored private let client: DaemonClient

    init(client: DaemonClient = DaemonClient()) {
        self.client = client
    }

    func loadAll(projectRoots: [String]) async {
        isLoading = true
        error = nil
        async let t: () = loadTemplates(projectRoots: projectRoots)
        async let a: () = loadAgentDefs(projectRoots: projectRoots)
        async let s: () = loadSwarmDefs(projectRoots: projectRoots)
        _ = await (t, a, s)
        isLoading = false
    }

    func loadTemplates(projectRoots: [String]) async {
        do {
            var merged: [String: SavedPrompt] = [:]
            for root in projectRoots {
                let projectName = URL(fileURLWithPath: root).lastPathComponent
                let response = try await client.send(.listTemplates(projectRoot: root))
                if case .templateList(let templates) = response {
                    for info in templates {
                        var prompt = SavedPrompt(from: info)
                        if info.source != "global" {
                            prompt.projectRoot = root
                            prompt.projectName = projectName
                        }
                        let key = prompt.id
                        if merged[key] == nil {
                            merged[key] = prompt
                        }
                    }
                }
            }
            prompts = Array(merged.values).sorted { $0.name < $1.name }
            if selectedPromptId == nil, let first = prompts.first {
                selectedPromptId = first.id
            }
        } catch {
            self.error = "Failed to load templates: \(error.localizedDescription)"
        }
    }

    func loadAgentDefs(projectRoots: [String]) async {
        do {
            var merged: [String: AgentDefinition] = [:]
            for root in projectRoots {
                let projectName = URL(fileURLWithPath: root).lastPathComponent
                let response = try await client.send(.listAgentDefs(projectRoot: root))
                if case .agentDefList(let defs) = response {
                    for info in defs {
                        var def = AgentDefinition(from: info)
                        if info.scope != "global" {
                            def.projectRoot = root
                            def.projectName = projectName
                        }
                        let key = def.id
                        if merged[key] == nil {
                            merged[key] = def
                        }
                    }
                }
            }
            agents = Array(merged.values).sorted { $0.name < $1.name }
            if selectedAgentId == nil, let first = agents.first {
                selectedAgentId = first.id
            }
        } catch {
            self.error = "Failed to load agent defs: \(error.localizedDescription)"
        }
    }

    func loadSwarmDefs(projectRoots: [String]) async {
        do {
            var merged: [String: SwarmDefinition] = [:]
            for root in projectRoots {
                let projectName = URL(fileURLWithPath: root).lastPathComponent
                let response = try await client.send(.listSwarmDefs(projectRoot: root))
                if case .swarmDefList(let defs) = response {
                    for info in defs {
                        var def = SwarmDefinition(from: info)
                        if info.scope != "global" {
                            def.projectRoot = root
                            def.projectName = projectName
                        }
                        let key = def.id
                        if merged[key] == nil {
                            merged[key] = def
                        }
                    }
                }
            }
            swarms = Array(merged.values).sorted { $0.name < $1.name }
            if selectedSwarmId == nil, let first = swarms.first {
                selectedSwarmId = first.id
            }
        } catch {
            self.error = "Failed to load swarm defs: \(error.localizedDescription)"
        }
    }

    func loadPromptDetail(projectRoot: String, name: String) async {
        do {
            let response = try await client.send(.getTemplate(projectRoot: projectRoot, name: name))
            if case .templateDetail(
                let detailName, let description, let agent, let body,
                let source, let variables, let command) = response
            {
                if let index = prompts.firstIndex(where: { $0.name == detailName && $0.source == source }) {
                    prompts[index].body = body
                    prompts[index].description = description
                    prompts[index].agent = agent
                    prompts[index].variables = variables
                    prompts[index].command = command
                }
            }
        } catch {
            self.error = "Failed to load prompt detail: \(error.localizedDescription)"
        }
    }

    func saveTemplate(
        projectRoot: String, projectRoots: [String],
        name: String, description: String, agent: String, body: String, scope: String,
        command: String? = nil
    ) async {
        do {
            _ = try await client.send(
                .saveTemplate(
                    projectRoot: projectRoot, name: name, description: description, agent: agent, body: body,
                    scope: scope, command: command))
            await loadTemplates(projectRoots: projectRoots)
            await loadPromptDetail(projectRoot: projectRoot, name: name)
        } catch {
            self.error = "Failed to save template: \(error.localizedDescription)"
        }
    }

    func deleteTemplate(projectRoot: String, projectRoots: [String], name: String, scope: String) async {
        do {
            _ = try await client.send(.deleteTemplate(projectRoot: projectRoot, name: name, scope: scope))
            await loadTemplates(projectRoots: projectRoots)
        } catch {
            self.error = "Failed to delete template: \(error.localizedDescription)"
        }
    }

    func saveAgentDef(projectRoot: String, projectRoots: [String], def: AgentDefinition) async {
        do {
            _ = try await client.send(
                .saveAgentDef(
                    projectRoot: projectRoot,
                    name: def.name,
                    agentType: def.agentType,
                    template: def.template,
                    inlinePrompt: def.inlinePrompt,
                    tags: def.tags,
                    scope: def.scope,
                    availableInCommandDialog: def.availableInCommandDialog,
                    icon: def.icon,
                    command: def.command
                ))
            await loadAgentDefs(projectRoots: projectRoots)
        } catch {
            self.error = "Failed to save agent def: \(error.localizedDescription)"
        }
    }

    func deleteAgentDef(projectRoot: String, projectRoots: [String], name: String, scope: String) async {
        do {
            _ = try await client.send(.deleteAgentDef(projectRoot: projectRoot, name: name, scope: scope))
            await loadAgentDefs(projectRoots: projectRoots)
        } catch {
            self.error = "Failed to delete agent def: \(error.localizedDescription)"
        }
    }

    func saveSwarmDef(projectRoot: String, projectRoots: [String], def: SwarmDefinition) async {
        do {
            _ = try await client.send(
                .saveSwarmDef(
                    projectRoot: projectRoot,
                    name: def.name,
                    worktreeCount: def.worktreeCount,
                    worktreeTemplate: def.worktreeTemplate,
                    roster: def.roster.map {
                        SwarmRosterEntryPayload(agentDef: $0.agentDef, role: $0.role, quantity: $0.quantity)
                    },
                    includeTerminal: def.includeTerminal,
                    scope: def.scope
                ))
            await loadSwarmDefs(projectRoots: projectRoots)
        } catch {
            self.error = "Failed to save swarm def: \(error.localizedDescription)"
        }
    }

    func deleteSwarmDef(projectRoot: String, projectRoots: [String], name: String, scope: String) async {
        do {
            _ = try await client.send(.deleteSwarmDef(projectRoot: projectRoot, name: name, scope: scope))
            await loadSwarmDefs(projectRoots: projectRoots)
        } catch {
            self.error = "Failed to delete swarm def: \(error.localizedDescription)"
        }
    }

    func runSwarm(projectRoot: String, name: String, vars: [String: String] = [:]) async {
        do {
            let response = try await client.send(.runSwarm(projectRoot: projectRoot, swarmName: name, vars: vars))
            if case .runSwarmResult(let agents) = response {
                print("[AgentsHubState] Spawned \(agents.count) agents: \(agents)")
            }
        } catch {
            self.error = "Failed to run swarm: \(error.localizedDescription)"
        }
    }
}
