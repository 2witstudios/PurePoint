import Testing
import Foundation
@testable import PurePoint

struct DaemonWorkspaceServiceTests {

    // Helper: decode a WorktreeEntry array from JSON
    private func decodeWorktrees(_ json: String) throws -> [WorktreeEntry] {
        try JSONDecoder().decode([WorktreeEntry].self, from: json.data(using: .utf8)!)
    }

    @Test func testParseWorktreesValid() throws {
        let entries = try decodeWorktrees(
            """
            [{"id":"wt-1","name":"feature-auth","path":"/tmp/project/.pu/worktrees/wt-1","branch":"pu/feature-auth","status":"active","agents":{"ag-1":{"id":"ag-1","name":"claude-1","agentType":"claude","status":"running","prompt":"add auth","startedAt":"2026-01-01T00:00:00Z"}},"createdAt":"2026-01-01T00:00:00Z"}]
            """)

        let models = DaemonWorkspaceService.parseWorktrees(entries)
        #expect(models.count == 1)
        #expect(models[0].id == "wt-1")
        #expect(models[0].name == "feature-auth")
        #expect(models[0].branch == "pu/feature-auth")
        #expect(models[0].status == "active")
        #expect(models[0].agents.count == 1)
        #expect(models[0].agents[0].id == "ag-1")
    }

    @Test func testParseWorktreesMissingFieldsFailsDecode() {
        // Missing required "branch" field — entire status_report fails to decode
        let json = """
            {"type":"status_report","worktrees":[{"id":"wt-1","name":"test","path":"/tmp","status":"active","agents":{},"createdAt":"2026-01-01T00:00:00Z"}],"agents":[]}
            """.data(using: .utf8)!

        let response = DaemonClient.parse(json)
        guard case .unknown(let type) = response else {
            Issue.record("expected parse_error for malformed worktree, got \(response)")
            return
        }
        #expect(type == "parse_error")
    }

    @Test func testParseWorktreesEmpty() {
        let models = DaemonWorkspaceService.parseWorktrees([])
        #expect(models.isEmpty)
    }

    @Test func testParseWorktreesNoAgents() throws {
        let entries = try decodeWorktrees(
            """
            [{"id":"wt-1","name":"empty-wt","path":"/tmp/project/.pu/worktrees/wt-1","branch":"pu/empty","status":"active","agents":{},"createdAt":"2026-01-01T00:00:00Z"}]
            """)

        let models = DaemonWorkspaceService.parseWorktrees(entries)
        #expect(models.count == 1)
        #expect(models[0].agents.isEmpty)
    }
}
