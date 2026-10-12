import Foundation
import Testing
@testable import PurePoint

@MainActor @Suite struct PointGuardSpawnRoutingChecks {
    @Test func splitSpawnRequestsUsePiFolderForTerminalsBuiltInsAndCustomCommands() throws {
        let root = NSTemporaryDirectory() + "pg-spawn-routing-" + UUID().uuidString
        let registry = WorkspaceRegistry()
        let state = AppState(startProject: { _ in })
        let saved = UserDefaults.standard.object(forKey: "PurePointPointGuardFolders")
        defer {
            UserDefaults.standard.set(saved, forKey: "PurePointPointGuardFolders")
            registry.forgetProject(root)
            try? FileManager.default.removeItem(atPath: root)
        }
        state.registry = registry
        state.updatePointGuardRoot(root)
        state.activeProjectRoot = "/different-project"
        let id = try #require(state.activatePointGuardWorkspace())
        let project = try #require(state.projectState(forRoot: root))
        for (index, launch) in [("terminal", nil), ("claude", nil), ("codex", nil),
                                 ("terminal", Optional("my-custom-agent"))].enumerated() {
            let leaf = try #require(registry.workspace(id: id)?.focusedLeafId)
            registry.split(workspaceId: id, leafId: leaf, axis: index % 2 == 0 ? .vertical : .horizontal)
            let sid = try #require(registry.workspace(id: id)?.focusedSurface?.id)
            project.spawnAgentForSurface(agent: launch.0, prompt: "hello", workspaceId: id,
                                         surfaceId: sid, name: "Fix Auth \(index)", command: launch.1)
            let request = try #require(project.requests.last)
            let json = try #require(JSONSerialization.jsonObject(with: JSONEncoder().encode(request)) as? [String: Any])
            #expect(json["project_root"] as? String == root)
            #expect(json["root"] as? Bool == true)
            #expect(json["worktree"] == nil)
            #expect(json["agent"] as? String == launch.0)
            #expect(json["name"] as? String == "Fix Auth \(index)")
            #expect(json["command"] as? String == launch.1)
            #expect(registry.workspace(id: id)?.focusedSurface?.content == .agent("ag-spawned"))
        }
    }
}
