import Testing
import Foundation
@testable import PurePoint

struct DaemonClientParseTests {

    @Test func testParseHealthReport() {
        let json = """
        {"type":"health_report","pid":1234,"uptime_seconds":60,"protocol_version":1,"agent_count":3}
        """.data(using: .utf8)!

        let response = DaemonClient.parse(json)
        guard case .healthReport(let pid, let uptime, let version, let count) = response else {
            Issue.record("expected healthReport, got \(response)")
            return
        }
        #expect(pid == 1234)
        #expect(uptime == 60)
        #expect(version == 1)
        #expect(count == 3)
    }

    @Test func testParseStatusReport() {
        let json = """
        {"type":"status_report","worktrees":[],"agents":[{"id":"ag-1","name":"test","status":"running"}]}
        """.data(using: .utf8)!

        let response = DaemonClient.parse(json)
        guard case .statusReport(let worktrees, let agents) = response else {
            Issue.record("expected statusReport, got \(response)")
            return
        }
        #expect(worktrees.isEmpty)
        #expect(agents.count == 1)
        #expect(agents[0].id == "ag-1")
        #expect(agents[0].name == "test")
        #expect(agents[0].status == "running")
    }

    @Test func testParseStatusReportWithAgentOptionalFields() {
        let json = """
        {"type":"status_report","worktrees":[],"agents":[{"id":"ag-1","name":"test","status":"completed","pid":9876,"exit_code":0,"idle_seconds":120,"worktree_id":"wt-1"}]}
        """.data(using: .utf8)!

        let response = DaemonClient.parse(json)
        guard case .statusReport(_, let agents) = response else {
            Issue.record("expected statusReport, got \(response)")
            return
        }
        #expect(agents.count == 1)
        #expect(agents[0].pid == 9876)
        #expect(agents[0].exitCode == 0)
        #expect(agents[0].idleSeconds == 120)
        #expect(agents[0].worktreeId == "wt-1")
    }

    @Test func testParseStatusReportWithWorktrees() {
        let json = """
        {"type":"status_report","worktrees":[{"id":"wt-1","name":"feature-auth","path":"/tmp/wt","branch":"pu/feature-auth","status":"active","agents":{"ag-1":{"id":"ag-1","name":"claude-1","agentType":"claude","status":"running","prompt":"add auth","startedAt":"2026-01-01T00:00:00Z"}},"createdAt":"2026-01-01T00:00:00Z"}],"agents":[]}
        """.data(using: .utf8)!

        let response = DaemonClient.parse(json)
        guard case .statusReport(let worktrees, let agents) = response else {
            Issue.record("expected statusReport, got \(response)")
            return
        }
        #expect(worktrees.count == 1)
        #expect(worktrees[0].id == "wt-1")
        #expect(worktrees[0].branch == "pu/feature-auth")
        #expect(worktrees[0].agents.count == 1)
        #expect(worktrees[0].agents["ag-1"]?.name == "claude-1")
        #expect(agents.isEmpty)
    }

    @Test func testParseAttachReady() {
        let json = """
        {"type":"attach_ready","buffered_bytes":4096}
        """.data(using: .utf8)!

        let response = DaemonClient.parse(json)
        guard case .attachReady(let bytes) = response else {
            Issue.record("expected attachReady, got \(response)")
            return
        }
        #expect(bytes == 4096)
    }

    @Test func testParseOutputDecodesHexData() {
        // "hello" = 68656c6c6f
        let json = """
        {"type":"output","agent_id":"ag-1","data":"68656c6c6f"}
        """.data(using: .utf8)!

        let response = DaemonClient.parse(json)
        guard case .output(let agentId, let data) = response else {
            Issue.record("expected output, got \(response)")
            return
        }
        #expect(agentId == "ag-1")
        #expect(data == Data("hello".utf8))
    }

    @Test func testParseError() {
        let json = """
        {"type":"error","code":"AGENT_NOT_FOUND","message":"no such agent"}
        """.data(using: .utf8)!

        let response = DaemonClient.parse(json)
        guard case .error(let code, let message) = response else {
            Issue.record("expected error, got \(response)")
            return
        }
        #expect(code == "AGENT_NOT_FOUND")
        #expect(message == "no such agent")
    }

    @Test func testParseOk() {
        let json = """
        {"type":"ok"}
        """.data(using: .utf8)!

        let response = DaemonClient.parse(json)
        guard case .ok = response else {
            Issue.record("expected ok, got \(response)")
            return
        }
    }

    @Test func testParseShuttingDown() {
        let json = """
        {"type":"shutting_down"}
        """.data(using: .utf8)!

        let response = DaemonClient.parse(json)
        guard case .shuttingDown = response else {
            Issue.record("expected shuttingDown, got \(response)")
            return
        }
    }

    @Test func testParseUnknownType() {
        let json = """
        {"type":"future_response_type","data":123}
        """.data(using: .utf8)!

        let response = DaemonClient.parse(json)
        guard case .unknown(let type) = response else {
            Issue.record("expected unknown, got \(response)")
            return
        }
        #expect(type == "future_response_type")
    }

    @Test func testParseMalformedJSON() {
        let json = "not valid json".data(using: .utf8)!

        let response = DaemonClient.parse(json)
        guard case .unknown(let type) = response else {
            Issue.record("expected unknown for malformed JSON, got \(response)")
            return
        }
        #expect(type == "parse_error")
    }

    // MARK: - Phase 5: Template / AgentDef / SwarmDef / RunSwarm

    @Test func testParseTemplateList() {
        let json = """
        {"type":"template_list","templates":[{"name":"review","description":"Code review","agent":"claude","source":"local","variables":["BRANCH"]}]}
        """.data(using: .utf8)!

        let response = DaemonClient.parse(json)
        guard case .templateList(let templates) = response else {
            Issue.record("expected templateList, got \(response)")
            return
        }
        #expect(templates.count == 1)
        #expect(templates[0].name == "review")
        #expect(templates[0].description == "Code review")
        #expect(templates[0].agent == "claude")
        #expect(templates[0].source == "local")
        #expect(templates[0].variables == ["BRANCH"])
    }

    @Test func testParseTemplateDetail() {
        let json = """
        {"type":"template_detail","name":"review","description":"Code review","agent":"claude","body":"Review {{BRANCH}}.","source":"local","variables":["BRANCH"]}
        """.data(using: .utf8)!

        let response = DaemonClient.parse(json)
        guard case .templateDetail(let name, let description, let agent, let body, let source, let variables) = response else {
            Issue.record("expected templateDetail, got \(response)")
            return
        }
        #expect(name == "review")
        #expect(description == "Code review")
        #expect(agent == "claude")
        #expect(body == "Review {{BRANCH}}.")
        #expect(source == "local")
        #expect(variables == ["BRANCH"])
    }

    @Test func testParseAgentDefList() {
        let json = """
        {"type":"agent_def_list","agent_defs":[{"name":"reviewer","agent_type":"claude","template":"review","tags":["review"],"scope":"local","available_in_command_dialog":true,"icon":"shield"}]}
        """.data(using: .utf8)!

        let response = DaemonClient.parse(json)
        guard case .agentDefList(let agentDefs) = response else {
            Issue.record("expected agentDefList, got \(response)")
            return
        }
        #expect(agentDefs.count == 1)
        #expect(agentDefs[0].name == "reviewer")
        #expect(agentDefs[0].agentType == "claude")
        #expect(agentDefs[0].template == "review")
        #expect(agentDefs[0].tags == ["review"])
        #expect(agentDefs[0].scope == "local")
        #expect(agentDefs[0].availableInCommandDialog == true)
        #expect(agentDefs[0].icon == "shield")
    }

    @Test func testParseAgentDefListWithNulls() {
        let json = """
        {"type":"agent_def_list","agent_defs":[{"name":"basic","agent_type":"claude","template":null,"tags":[],"scope":"global","available_in_command_dialog":false,"icon":null}]}
        """.data(using: .utf8)!

        let response = DaemonClient.parse(json)
        guard case .agentDefList(let agentDefs) = response else {
            Issue.record("expected agentDefList, got \(response)")
            return
        }
        #expect(agentDefs.count == 1)
        #expect(agentDefs[0].name == "basic")
        #expect(agentDefs[0].template == nil)
        #expect(agentDefs[0].tags.isEmpty)
        #expect(agentDefs[0].availableInCommandDialog == false)
        #expect(agentDefs[0].icon == nil)
    }

    @Test func testParseSwarmDefList() {
        let json = """
        {"type":"swarm_def_list","swarm_defs":[{"name":"full-stack","worktree_count":3,"worktree_template":"feature","roster":[{"agent_def":"reviewer","role":"review","quantity":2}],"include_terminal":true,"scope":"local"}]}
        """.data(using: .utf8)!

        let response = DaemonClient.parse(json)
        guard case .swarmDefList(let swarmDefs) = response else {
            Issue.record("expected swarmDefList, got \(response)")
            return
        }
        #expect(swarmDefs.count == 1)
        #expect(swarmDefs[0].name == "full-stack")
        #expect(swarmDefs[0].worktreeCount == 3)
        #expect(swarmDefs[0].worktreeTemplate == "feature")
        #expect(swarmDefs[0].roster.count == 1)
        #expect(swarmDefs[0].roster[0].agentDef == "reviewer")
        #expect(swarmDefs[0].roster[0].role == "review")
        #expect(swarmDefs[0].roster[0].quantity == 2)
        #expect(swarmDefs[0].includeTerminal == true)
        #expect(swarmDefs[0].scope == "local")
    }

    @Test func testParseRunSwarmResult() {
        let json = """
        {"type":"run_swarm_result","spawned_agents":["ag-abc","ag-def"]}
        """.data(using: .utf8)!

        let response = DaemonClient.parse(json)
        guard case .runSwarmResult(let spawnedAgents) = response else {
            Issue.record("expected runSwarmResult, got \(response)")
            return
        }
        #expect(spawnedAgents == ["ag-abc", "ag-def"])
    }

    @Test func testParseTemplateListEmpty() {
        let json = """
        {"type":"template_list","templates":[]}
        """.data(using: .utf8)!

        let response = DaemonClient.parse(json)
        guard case .templateList(let templates) = response else {
            Issue.record("expected templateList, got \(response)")
            return
        }
        #expect(templates.isEmpty)
    }
}
