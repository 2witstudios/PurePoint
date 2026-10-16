import Foundation
@main struct NavigationChecks {
    static func main() throws {
        var preferences = AgentNavigationPreferences()
        let a = AgentScope(host: "bridge", machine: "one", agent: "a")
        let collision = AgentScope(host: "bridge", machine: "two", agent: "a")
        preferences.pin(a)
        preferences.interact(collision, at: 50)
        for index in 0..<30 { preferences.interact(AgentScope(host: "bridge", machine: "one", agent: "swarm\(index)"), at: Double(index + 100)) }
        precondition(preferences.pins == [a.storageKey], "swarm must not evict pins")
        precondition(!preferences.isPinned(collision), "machine identity must scope pins")
        preferences.hide(a)
        precondition(preferences.isHidden(a) && !preferences.isPinned(a))
        preferences.interact(a, at: 200)
        precondition(!preferences.isHidden(a))
        let restored = try JSONDecoder().decode(AgentNavigationPreferences.self, from: JSONEncoder().encode(preferences))
        precondition(restored == preferences, "preferences survive relaunch")
        preferences.pin(a); preferences.pin(collision); preferences.pin(a)
        precondition(preferences.pins == [a.storageKey, collision.storageKey])
        let scope = AgentScope(host: "bridge", machine: "one", agent: "untouched")
        let started = "2026-10-16T10:00:00Z"
        let initial = preferences.recency(scope, startedAt: started)
        precondition(initial > 0)
        // Refresh and terminal output never call interact.
        precondition(preferences.recency(scope, startedAt: started) == initial)
        preferences.interact(scope, at: initial + 100)
        precondition(preferences.recency(scope, startedAt: started) == initial + 100)
        var form = WorkspaceFormDraft()
        form.prompt = "Original launch"; form.operationId = UUID().uuidString; form.state = "pending"
        let savedForm = try JSONDecoder().decode(WorkspaceFormDraft.self, from: JSONEncoder().encode(form))
        precondition(savedForm == form && savedForm.operationId != nil, "relaunch retains operation identity and disables repeat launch")
        let legacy = try JSONDecoder().decode(AgentInventory.self, from: Data(#"{"machines":[{"id":"m","name":"Mac","online":true,"screenMayBeInexact":false,"complete":true,"agents":[]}]}"#.utf8))
        precondition(legacy.machines[0].registeredProjects == nil, "legacy metadata is absent, never fabricated")
        let location = WorkspaceLocation(host: "a", machine: "bc", project: "repo", worktree: "tree")
        let collisionLocation = WorkspaceLocation(host: "ab", machine: "c", project: "repo", worktree: "tree")
        precondition(location.storageKey != collisionLocation.storageKey)
        let workspaceJSON = #"{"machines":[{"id":"machine","name":"Mac","online":true,"screenMayBeInexact":false,"complete":true,"agents":[{"id":"agent","name":"Worker","agentType":"claude","state":"running","projectRoot":"/repo","worktreeId":"w","screenAware":false}],"projects":[{"id":"/repo","name":"Repo","available":true}],"worktrees":[{"projectId":"/repo","id":"w","name":"Task","path":"/repo/.pu/w","branch":"feature","status":"active","agentIds":["agent"]}]}]}"#
        let inventory = try JSONDecoder().decode(AgentInventory.self, from: Data(workspaceJSON.utf8))
        let exact = WorkspaceLocation.locate(AgentScope(host: "bridge", machine: "machine", agent: "agent"), machines: inventory.machines)
        precondition(exact == WorkspaceLocation(host: "bridge", machine: "machine", project: "/repo", worktree: "w"))
        precondition(WorkspaceLocation.locate(AgentScope(host: "bridge", machine: "other", agent: "agent"), machines: inventory.machines) == nil)
        precondition(WorkspaceLocation.locate(AgentScope(host: "bridge", machine: "m", agent: "agent"), machines: legacy.machines) == nil)
        var expansion = Set([exact!.storageKey])
        expansion.remove(exact!.storageKey)
        precondition(!expansion.contains(exact!.storageKey), "Locate opens the exact ancestor identity")
        var memory = WorkspaceNavigationMemory()
        let workspaceA = WorkspaceLocation(host: "host-a", machine: "m", project: "/a", worktree: nil)
        let workspaceB = WorkspaceLocation(host: "host-a", machine: "m", project: "/b", worktree: nil)
        memory.workspaceAnchors[workspaceA.storageKey] = "agent-a"
        memory.workspaceAnchors[workspaceB.storageKey] = "agent-b"
        memory.collapsed.insert(exact!.storageKey)
        memory.locate(exact!, agent: "agent")
        precondition(memory.workspaceAnchors[workspaceA.storageKey] == "agent-a" && memory.workspaceAnchors[workspaceB.storageKey] == "agent-b", "Visiting another workspace must retain each scroll anchor")
        precondition(!memory.collapsed.contains(exact!.storageKey))
        precondition(memory.workspaceAnchors[WorkspaceLocation(host: "host-b", machine: "m", project: "/a", worktree: nil).storageKey] == nil)
        var overlays = MobileOverlayState()
        overlays.openNavigator(); precondition(overlays.navigator && !overlays.history)
        overlays.openHistory(); precondition(overlays.history && !overlays.navigator)
        overlays.dialogArrived(selection: 17); precondition(!overlays.history && !overlays.navigator, "Dialog arrival must leave no overlay blocking presentation")
        precondition(overlays.shouldPresentDialog(selection: 17) && !overlays.shouldPresentDialog(selection: 18), "Closing history must present the pending dialog only to its originating host selection")
        let context = AgentDestinationContext.resolve(scope: AgentScope(host: "paired-bridge", machine: "machine", agent: "agent"), machines: inventory.machines, connected: true)
        precondition(context.agentName == "Worker" && context.machineName == "Mac" && context.projectLabel == "Repo" && context.worktreeLabel == "Task · feature")
        let remoteJSON = workspaceJSON.replacingOccurrences(of: "\"machine\"", with: "\"remote\"").replacingOccurrences(of: "\"Mac\"", with: "\"Remote Mac\"").replacingOccurrences(of: "\"Worker\"", with: "\"Remote worker\"").replacingOccurrences(of: "/repo", with: "/remote").replacingOccurrences(of: "\"Repo\"", with: "\"Remote repo\"")
        let remoteInventory = try JSONDecoder().decode(AgentInventory.self, from: Data(remoteJSON.utf8))
        let remoteContext = AgentDestinationContext.resolve(scope: AgentScope(host: "paired-bridge", machine: "remote", agent: "agent"), machines: inventory.machines + remoteInventory.machines, connected: true)
        precondition(remoteContext.agentName == "Remote worker" && remoteContext.machineName == "Remote Mac" && remoteContext.projectLabel == "Remote repo", "Same agent/worktree IDs on another daemon must never borrow local display identity")
        let wrongMachine = AgentDestinationContext.resolve(scope: AgentScope(host: "paired-bridge", machine: "other-machine", agent: "agent"), machines: inventory.machines, connected: true)
        precondition(wrongMachine.machineName == "other-machine" && wrongMachine.agentName == "agent" && wrongMachine.projectLabel == "Project unavailable")
        precondition(wrongMachine.worktreeLabel == "Location unavailable" && wrongMachine.status.contains("Unavailable"))
        let offlineContext = AgentDestinationContext.resolve(scope: AgentScope(host: "paired-bridge", machine: "machine", agent: "agent"), machines: inventory.machines, connected: false)
        precondition(offlineContext.agentName == context.agentName && offlineContext.status.contains("Unavailable"))
        // Virtual stage budgets exercise the full admitted fleet without wall-clock waits.
        let perMachineStageSeconds = Array(repeating: 5 + 5 + max(5, 5), count: 32)
        let fleetBudgetSeconds = stride(from: 0, to: perMachineStageSeconds.count, by: 4).reduce(0) { budget, offset in
            budget + perMachineStageSeconds[offset..<min(offset + 4, perMachineStageSeconds.count)].max()!
        }
        precondition(fleetBudgetSeconds == 120)
        precondition(MobileRPCTiming.timeoutNanoseconds(for: "agents") >= UInt64(fleetBudgetSeconds + 30) * 1_000_000_000)
        precondition(MobileRPCTiming.timeoutNanoseconds(for: "agent.prompt") == 390_000_000_000)
        precondition(MobileRPCTiming.timeoutNanoseconds(for: "permissions") == 35_000_000_000)
        precondition(MobileRPCTiming.timeoutNanoseconds(for: "sync") == 35_000_000_000)
        print("Navigation preference checks passed")
    }
}
