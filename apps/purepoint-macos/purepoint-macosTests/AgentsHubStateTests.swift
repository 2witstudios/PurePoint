import Testing
import Foundation
@testable import PurePoint

struct AgentsHubStateTests {

    @Test @MainActor func testInitialStateIsEmpty() {
        let state = AgentsHubState()

        #expect(state.prompts.isEmpty)
        #expect(state.agents.isEmpty)
        #expect(state.swarms.isEmpty)
        #expect(state.selectedPromptId == nil)
        #expect(state.selectedAgentId == nil)
        #expect(state.selectedSwarmId == nil)
        #expect(state.showingCreatePrompt == false)
        #expect(state.showingCreateAgent == false)
        #expect(state.showingCreateSwarm == false)
    }

    @Test @MainActor func testSelectedPromptReturnsNilWhenEmpty() {
        let state = AgentsHubState()

        #expect(state.selectedPrompt == nil)
    }

    @Test @MainActor func testSelectedPromptReturnsMatchingPrompt() {
        let state = AgentsHubState()
        state.prompts = [
            SavedPrompt(name: "review", description: "Review", agent: "claude", body: "", source: "local", variables: []),
            SavedPrompt(name: "deploy", description: "Deploy", agent: "claude", body: "", source: "global", variables: []),
        ]
        state.selectedPromptId = "deploy"

        #expect(state.selectedPrompt?.name == "deploy")
    }

    @Test @MainActor func testSelectedAgentReturnsMatchingAgent() {
        let state = AgentsHubState()
        state.agents = [
            AgentDefinition(name: "reviewer"),
            AgentDefinition(name: "fixer"),
        ]
        state.selectedAgentId = "fixer"

        #expect(state.selectedAgent?.name == "fixer")
    }

    @Test @MainActor func testSelectedSwarmReturnsMatchingSwarm() {
        let state = AgentsHubState()
        state.swarms = [
            SwarmDefinition(name: "full-stack"),
            SwarmDefinition(name: "lite"),
        ]
        state.selectedSwarmId = "lite"

        #expect(state.selectedSwarm?.name == "lite")
    }

    @Test @MainActor func testSelectedPromptReturnsNilForMismatchId() {
        let state = AgentsHubState()
        state.prompts = [
            SavedPrompt(name: "review", description: "Review", agent: "claude", body: "", source: "local", variables: []),
        ]
        state.selectedPromptId = "nonexistent"

        #expect(state.selectedPrompt == nil)
    }
}
