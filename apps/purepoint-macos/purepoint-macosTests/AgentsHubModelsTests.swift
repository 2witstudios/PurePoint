import Testing
import Foundation
@testable import PurePoint

struct AgentsHubModelsTests {

    // MARK: - SavedPrompt

    @Test func testSavedPromptFromTemplateInfo() {
        let json = """
            {"name":"review","description":"Code review","agent":"claude","source":"local","variables":["BRANCH"]}
            """.data(using: .utf8)!

        let info = try! JSONDecoder().decode(TemplateInfo.self, from: json)
        let prompt = SavedPrompt(from: info)

        #expect(prompt.id == "local:review")
        #expect(prompt.name == "review")
        #expect(prompt.description == "Code review")
        #expect(prompt.agent == "claude")
        #expect(prompt.body == "")
        #expect(prompt.source == "local")
        #expect(prompt.variables == ["BRANCH"])
    }

    @Test func testSavedPromptManualInit() {
        let prompt = SavedPrompt(
            name: "test",
            description: "A test",
            agent: "claude",
            body: "Do something.",
            source: "global",
            variables: ["VAR"]
        )

        #expect(prompt.id == "test")
        #expect(prompt.name == "test")
        #expect(prompt.body == "Do something.")
    }

    // MARK: - AgentDefinition

    @Test func testAgentDefinitionFromAgentDefInfo() {
        let json = """
            {"name":"reviewer","agent_type":"claude","template":"review","tags":["review"],"scope":"local","available_in_command_dialog":true,"icon":"shield"}
            """.data(using: .utf8)!

        let info = try! JSONDecoder().decode(AgentDefInfo.self, from: json)
        let def = AgentDefinition(from: info)

        #expect(def.id == "local:reviewer")
        #expect(def.name == "reviewer")
        #expect(def.agentType == "claude")
        #expect(def.template == "review")
        #expect(def.inlinePrompt == nil)
        #expect(def.tags == ["review"])
        #expect(def.scope == "local")
        #expect(def.availableInCommandDialog == true)
        #expect(def.icon == "shield")
    }

    @Test func testAgentDefinitionManualInit() {
        let def = AgentDefinition(name: "test-agent")

        #expect(def.id == "test-agent")
        #expect(def.agentType == "claude")
        #expect(def.template == nil)
        #expect(def.tags.isEmpty)
        #expect(def.scope == "local")
        #expect(def.availableInCommandDialog == true)
        #expect(def.icon == nil)
    }

    // MARK: - SwarmDefinition

    @Test func testSwarmDefinitionFromSwarmDefInfo() {
        let json = """
            {"name":"full-stack","worktree_count":3,"worktree_template":"feature","roster":[{"agent_def":"reviewer","role":"review","quantity":2}],"include_terminal":true,"scope":"local"}
            """.data(using: .utf8)!

        let info = try! JSONDecoder().decode(SwarmDefInfo.self, from: json)
        let swarm = SwarmDefinition(from: info)

        #expect(swarm.id == "local:full-stack")
        #expect(swarm.name == "full-stack")
        #expect(swarm.worktreeCount == 3)
        #expect(swarm.worktreeTemplate == "feature")
        #expect(swarm.roster.count == 1)
        #expect(swarm.roster[0].agentDef == "reviewer")
        #expect(swarm.roster[0].role == "review")
        #expect(swarm.roster[0].quantity == 2)
        #expect(swarm.includeTerminal == true)
        #expect(swarm.scope == "local")
    }

    @Test func testSwarmDefinitionTotalAgents() {
        let swarm = SwarmDefinition(
            name: "test",
            worktreeCount: 3,
            worktreeTemplate: "feature",
            roster: [
                SwarmRosterItem(agentDef: "reviewer", role: "review", quantity: 2),
                SwarmRosterItem(agentDef: "fixer", role: "fix", quantity: 1),
            ],
            includeTerminal: false,
            scope: "local"
        )

        #expect(swarm.totalAgents == 9)  // 3 * (2 + 1)
    }

    @Test func testSwarmDefinitionManualInit() {
        let swarm = SwarmDefinition(name: "empty")

        #expect(swarm.id == "empty")
        #expect(swarm.worktreeCount == 1)
        #expect(swarm.roster.isEmpty)
        #expect(swarm.includeTerminal == false)
        #expect(swarm.scope == "local")
    }

    // MARK: - PromptScopeChoice

    @Test func testPromptScopeChoiceWireValues() {
        #expect(PromptScopeChoice.global.wireValue == "global")
        #expect(PromptScopeChoice.project.wireValue == "local")
    }

    @Test func testPromptScopeChoiceTitles() {
        #expect(PromptScopeChoice.global.title == "Global")
        #expect(PromptScopeChoice.project.title == "Project")
    }

    // MARK: - AgentPromptSourceMode

    @Test func testAgentPromptSourceModeTitles() {
        #expect(AgentPromptSourceMode.library.title == "From Prompts")
        #expect(AgentPromptSourceMode.inline.title == "Inline")
    }
}
