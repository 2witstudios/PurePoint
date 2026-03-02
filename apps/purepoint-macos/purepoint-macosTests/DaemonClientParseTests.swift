import Testing
import Foundation
@testable import purepoint_macos

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
}
