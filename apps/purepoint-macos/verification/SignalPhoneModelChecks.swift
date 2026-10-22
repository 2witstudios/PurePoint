import Foundation

@main enum SignalPhoneModelChecks {
    @MainActor static func main() async throws {
        let root = URL(fileURLWithPath: CommandLine.arguments[1])
        let data = try Data(contentsOf: root.appendingPathComponent("crates/pu-core/tests/fixtures/signals-inventory-v1.json"))
        let inventory = try JSONSerialization.jsonObject(with: data) as! [String: Any]
        let source = (inventory["agents"] as! [[String: Any]])[0]
        var signal = source["signal"] as! [String: Any]
        let now = Int64(Date().timeIntervalSince1970 * 1000)
        signal["observation"] = ["state": "current", "last_received_at_ms": now - 1, "expires_at_ms": now + 60_000]
        var agent: [String: Any] = ["id": "ag-root", "name": "Root", "agentType": "fixture", "state": "running", "screenAware": false, "signal": signal]
        var online = true
        func payload() throws -> JSONValue {
            try JSONDecoder().decode(JSONValue.self, from: JSONSerialization.data(withJSONObject: ["machines": [["id": "local", "name": "Mac", "online": online, "screenMayBeInexact": false, "complete": true, "agents": [agent, agent]]]]))
        }
        let suite = "pu-native-signal-" + UUID().uuidString
        let defaults = UserDefaults(suiteName: suite)!
        defer { defaults.removePersistentDomain(forName: suite) }
        let model = AgentControlModel(defaults: defaults)
        model.transport(host: "fixture", connected: true) { operation, _ in
            if operation == "permissions" {
                return try JSONDecoder().decode(JSONValue.self, from: Data(#"{"terminals":"view"}"#.utf8))
            }
            return try payload()
        }
        for _ in 0..<100 where model.machines.isEmpty { try await Task.sleep(for: .milliseconds(10)) }
        precondition(model.signalAttentionCount == 1, "multiple request/agent rows count once")
        model.setInventoryForeground(false)
        precondition(model.signalAttentionCount == 0)
        model.setInventoryForeground(true)
        precondition(model.signalAttentionCount == 1)
        online = false
        _ = await model.refresh()
        precondition(model.signalAttentionCount == 0)
        online = true
        agent["worktreeId"] = "wrong-worktree"
        _ = await model.refresh()
        precondition(model.signalAttentionCount == 0)
        model.transport(host: "fixture", connected: false)
        precondition(model.signalAttentionCount == 0)
        print("Foreground phone badge model passed: distinct identity, offline/background withdrawal, worktree binding and disconnect")
    }
}
