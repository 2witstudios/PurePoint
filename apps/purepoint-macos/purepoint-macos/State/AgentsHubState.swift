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

    var selectedPrompt: SavedPrompt? {
        prompts.first { $0.id == selectedPromptId }
    }
    var selectedAgent: AgentDefinition? {
        agents.first { $0.id == selectedAgentId }
    }
    var selectedSwarm: SwarmDefinition? {
        swarms.first { $0.id == selectedSwarmId }
    }

    @ObservationIgnored private let client = DaemonClient()

    func loadAll(projectRoot: String) async {
        await loadTemplates(projectRoot: projectRoot)
        await loadAgentDefs(projectRoot: projectRoot)
        await loadSwarmDefs(projectRoot: projectRoot)
    }

    func loadTemplates(projectRoot: String) async {
        do {
            let response = try await client.send(.listTemplates(projectRoot: projectRoot))
            if case .templateList(let templates) = response {
                prompts = templates.map { SavedPrompt(from: $0) }
                if selectedPromptId == nil, let first = prompts.first {
                    selectedPromptId = first.id
                }
            }
        } catch {
            print("[AgentsHubState] loadTemplates error: \(error)")
        }
    }

    func loadAgentDefs(projectRoot: String) async {
        do {
            let response = try await client.send(.listAgentDefs(projectRoot: projectRoot))
            if case .agentDefList(let defs) = response {
                agents = defs.map { AgentDefinition(from: $0) }
                if selectedAgentId == nil, let first = agents.first {
                    selectedAgentId = first.id
                }
            }
        } catch {
            print("[AgentsHubState] loadAgentDefs error: \(error)")
        }
    }

    func loadSwarmDefs(projectRoot: String) async {
        do {
            let response = try await client.send(.listSwarmDefs(projectRoot: projectRoot))
            if case .swarmDefList(let defs) = response {
                swarms = defs.map { SwarmDefinition(from: $0) }
                if selectedSwarmId == nil, let first = swarms.first {
                    selectedSwarmId = first.id
                }
            }
        } catch {
            print("[AgentsHubState] loadSwarmDefs error: \(error)")
        }
    }

    func saveTemplate(projectRoot: String, name: String, description: String, agent: String, body: String, scope: String) async {
        do {
            _ = try await client.send(.saveTemplate(projectRoot: projectRoot, name: name, description: description, agent: agent, body: body, scope: scope))
            await loadTemplates(projectRoot: projectRoot)
        } catch {
            print("[AgentsHubState] saveTemplate error: \(error)")
        }
    }

    func deleteTemplate(projectRoot: String, name: String, scope: String) async {
        do {
            _ = try await client.send(.deleteTemplate(projectRoot: projectRoot, name: name, scope: scope))
            await loadTemplates(projectRoot: projectRoot)
        } catch {
            print("[AgentsHubState] deleteTemplate error: \(error)")
        }
    }

    func saveAgentDef(projectRoot: String, def: AgentDefinition) async {
        do {
            _ = try await client.send(.saveAgentDef(
                projectRoot: projectRoot,
                name: def.name,
                agentType: def.agentType,
                template: def.template,
                inlinePrompt: def.inlinePrompt,
                tags: def.tags,
                scope: def.scope,
                availableInCommandDialog: def.availableInCommandDialog,
                icon: def.icon
            ))
            await loadAgentDefs(projectRoot: projectRoot)
        } catch {
            print("[AgentsHubState] saveAgentDef error: \(error)")
        }
    }

    func deleteAgentDef(projectRoot: String, name: String, scope: String) async {
        do {
            _ = try await client.send(.deleteAgentDef(projectRoot: projectRoot, name: name, scope: scope))
            await loadAgentDefs(projectRoot: projectRoot)
        } catch {
            print("[AgentsHubState] deleteAgentDef error: \(error)")
        }
    }

    func saveSwarmDef(projectRoot: String, def: SwarmDefinition) async {
        do {
            _ = try await client.send(.saveSwarmDef(
                projectRoot: projectRoot,
                name: def.name,
                worktreeCount: def.worktreeCount,
                worktreeTemplate: def.worktreeTemplate,
                roster: def.roster.map { SwarmRosterEntryPayload(agentDef: $0.agentDef, role: $0.role, quantity: $0.quantity) },
                includeTerminal: def.includeTerminal,
                scope: def.scope
            ))
            await loadSwarmDefs(projectRoot: projectRoot)
        } catch {
            print("[AgentsHubState] saveSwarmDef error: \(error)")
        }
    }

    func deleteSwarmDef(projectRoot: String, name: String, scope: String) async {
        do {
            _ = try await client.send(.deleteSwarmDef(projectRoot: projectRoot, name: name, scope: scope))
            await loadSwarmDefs(projectRoot: projectRoot)
        } catch {
            print("[AgentsHubState] deleteSwarmDef error: \(error)")
        }
    }

    func runSwarm(projectRoot: String, name: String, vars: [String: String] = [:]) async {
        do {
            let response = try await client.send(.runSwarm(projectRoot: projectRoot, swarmName: name, vars: vars))
            if case .runSwarmResult(let agents) = response {
                print("[AgentsHubState] Spawned \(agents.count) agents: \(agents)")
            }
        } catch {
            print("[AgentsHubState] runSwarm error: \(error)")
        }
    }
}
