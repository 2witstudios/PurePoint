import Foundation
#if !PHONE_CONTROL_CHECKS
    @testable import PurePoint
#endif

@MainActor enum AgentControlScenarios {
    @MainActor final class Bridge {
        var opens: [CheckedContinuation<JSONValue, Error>] = []
        var prompts: [CheckedContinuation<JSONValue, Error>] = []
        var operations: [(String, [String: Any])] = []
        var managementStatuses: [CheckedContinuation<JSONValue, Error>] = []
        var inventory = ""
        var holdInventory = false
        var inventories: [CheckedContinuation<JSONValue, Error>] = []
        static func permissionJSON() throws -> JSONValue { try AgentControlScenarios.json(#"{"terminals":"control","workspaceManagement":true,"workspace":{"version":1,"management":true}}"#) }
        func request(_ op: String, _ fields: [String: Any]) async throws -> JSONValue {
            operations.append((op, fields))
            switch op {
            case "permissions": return try Self.permissionJSON()
            case "workspace.rename": return try AgentControlScenarios.json(#"{"operationId":"operation-one","state":"succeeded"}"#)
            case "workspace.operation": return try await withCheckedThrowingContinuation { managementStatuses.append($0) }
            case "agents":
                if holdInventory { return try await withCheckedThrowingContinuation { inventories.append($0) } }
                return try JSONDecoder().decode(JSONValue.self, from: Data(inventory.utf8))
            case "terminal.open": return try await withCheckedThrowingContinuation { opens.append($0) }
            case "agent.prompt": return try await withCheckedThrowingContinuation { prompts.append($0) }
            default: return .object([:])
            }
        }
    }
    struct CheckFailure: LocalizedError { let errorDescription: String? }
    static func check(_ condition: @autoclosure () -> Bool, _ message: String) throws {
        if !condition() { throw CheckFailure(errorDescription: message) }
    }
    static func wait(_ predicate: () -> Bool) async throws {
        for _ in 0..<1000 { if predicate() { return }; await Task.yield() }
        throw CheckFailure(errorDescription: "Async fixture did not settle")
    }
    static func event(_ json: String) throws -> WireEnvelope {
        try JSONDecoder().decode(WireEnvelope.self, from: Data(json.utf8))
    }
    static func json(_ text: String) throws -> JSONValue {
        try JSONDecoder().decode(JSONValue.self, from: Data(text.utf8))
    }
    static func setup(interval: UInt64 = 5_000_000_000) async throws -> (AgentControlModel, Bridge, UserDefaults, String) {
        let suite = "phone-agent-checks-" + UUID().uuidString
        let defaults = UserDefaults(suiteName: suite)!
        let model = AgentControlModel(defaults: defaults, inventoryRefreshNanoseconds: interval), bridge = Bridge()
        bridge.inventory =
            #"{"machines":[{"id":"local","name":"Mac","online":true,"protocolVersion":10,"screenMayBeInexact":false,"error":null,"complete":true,"agents":[{"id":"a","name":"Agent","agentType":"claude","state":"waiting","projectRoot":"/repo","worktreeId":null,"screenAware":true,"phase":"input_ready","idleSeconds":3,"startedAt":""}]}]}"#
        model.transport(host: "host-a", connected: true, request: bridge.request)
        try await wait { !model.machines.isEmpty }
        return (model, bridge, defaults, suite)
    }
    static func open(_ model: AgentControlModel, _ bridge: Bridge) async throws {
        model.open(machine: model.machines[0], agent: model.machines[0].agents[0], context: model.contextToken)
        try await wait { !bridge.opens.isEmpty }
    }
    static func finishOpen(_ bridge: Bridge, id: String = "s") throws {
        bridge.opens.removeFirst().resume(
            returning: try json(
                "{\"streamId\":\"\(id)\",\"machine\":\"local\",\"agentId\":\"a\",\"screenMayBeInexact\":false}"))
    }

    static func allPromptOutcomesAndNoResend() async throws {
        for outcome in ["confirmed", "sent", "blocked", "failed"] {
            let (model, bridge, defaults, suite) = try await setup()
            try await open(model, bridge); try finishOpen(bridge)
            try await wait { model.streamId != nil }
            model.editDraft("Keep me", screen: model.screenToken); model.sendPrompt(screen: model.screenToken)
            try await wait { !bridge.prompts.isEmpty }
            bridge.prompts.removeFirst().resume(returning: try json("{\"outcome\":\"\(outcome)\",\"reason\":null}"))
            try await wait { model.composer.pending == nil }
            try check(model.composer.outcome?.rawValue == outcome, "Every bridge outcome must reach the composer")
            try check(
                model.draft == (["confirmed", "sent"].contains(outcome) ? "" : "Keep me"),
                "Only successful delivery clears the unchanged draft")
            model.transport(host: "host-a", connected: false)
            model.transport(host: "host-a", connected: true, request: bridge.request)
            try await wait { !model.machines.isEmpty }
            try check(
                bridge.operations.filter { $0.0 == "agent.prompt" }.count == 1, "Reconnect must never resend a prompt")
            model.transport(host: "", connected: false); defaults.removePersistentDomain(forName: suite)
        }
    }

    static func closeUsesOriginatingTransportAndNeverLeavesPendingDraft() async throws {
        let (model, first, defaults, suite) = try await setup()
        defer { model.transport(host: "", connected: false); defaults.removePersistentDomain(forName: suite) }
        try await open(model, first); try finishOpen(first)
        try await wait { model.streamId != nil }
        model.editDraft("Keep while leaving", screen: model.screenToken)
        model.sendPrompt(screen: model.screenToken)
        try await wait { !first.prompts.isEmpty }
        model.closeScreen()
        first.prompts.removeFirst().resume(returning: try json(#"{"outcome":"confirmed","reason":null}"#))
        try await open(model, first); try finishOpen(first, id: "again")
        try await wait { model.streamId == "again" }
        try check(
            model.composer.pending == nil && model.draft == "Keep while leaving",
            "Returning to a screen must not inherit a forever-pending submission or lose its uncertain text")
        let second = Bridge(); second.inventory = first.inventory
        model.transport(host: "host-b", connected: true, request: second.request)
        try await wait { first.operations.filter { $0.0 == "terminal.close" }.count == 2 }
        try check(
            !second.operations.contains { $0.0 == "terminal.close" },
            "Old stream must never close a same-named stream on another host")
    }

    static func failedOriginalAndNewerDraftSurviveNavigationHostChangeAndRelaunch() async throws {
        for outcome in ["failed", "blocked", "uncertain"] {
            let (model, bridge, defaults, suite) = try await setup()
            defer { model.transport(host: "", connected: false); defaults.removePersistentDomain(forName: suite) }
            try await open(model, bridge); try finishOpen(bridge)
            try await wait { model.streamId != nil }
            model.editDraft("Original submission", screen: model.screenToken)
            model.sendPrompt(screen: model.screenToken)
            try await wait { !bridge.prompts.isEmpty }
            model.editDraft("Newer draft", screen: model.screenToken)
            if outcome == "uncertain" { model.closeScreen() }
            bridge.prompts.removeFirst().resume(
                returning: try json(
                    "{\"outcome\":\"\(outcome == "uncertain" ? "confirmed" : outcome)\",\"reason\":\"Fixture result\"}")
            )
            if outcome != "uncertain" { try await wait { model.composer.pending == nil }; model.closeScreen() }
            try await open(model, bridge); try finishOpen(bridge, id: "reopened")
            try await wait { model.streamId == "reopened" }
            try check(
                model.draft == "Newer draft" && model.recoveries.contains { $0.text == "Original submission" },
                "Original failed/blocked/uncertain text and newer draft must survive screen exit")
            model.transport(host: "host-b", connected: true, request: bridge.request)
            try await wait { !model.machines.isEmpty }
            try await open(model, bridge); try finishOpen(bridge, id: "b")
            try await wait { model.streamId == "b" }
            try check(model.recoveries.isEmpty, "Recovery must never leak to another host")
            model.editDraft("B draft", screen: model.screenToken)
            model.transport(host: "host-a", connected: true, request: bridge.request)
            try await wait { !model.machines.isEmpty }
            try await open(model, bridge); try finishOpen(bridge, id: "a")
            try await wait { model.streamId == "a" }
            try check(
                model.draft == "Newer draft" && model.recoveries.contains { $0.text == "Original submission" },
                "Returning to origin must retain both texts")
            model.closeScreen()
            let relaunched = AgentControlModel(defaults: defaults)
            relaunched.transport(host: "host-a", connected: true, request: bridge.request)
            try await wait { !relaunched.machines.isEmpty }
            try await open(relaunched, bridge); try finishOpen(bridge, id: "relaunch")
            try await wait { relaunched.streamId == "relaunch" }
            try check(
                relaunched.draft == "Newer draft"
                    && relaunched.recoveries.contains { $0.text == "Original submission" },
                "Relaunch must hydrate both draft and recovery")
            let saved = relaunched.recoveries.first!
            relaunched.restoreRecovery(saved.id, screen: relaunched.screenToken)
            try check(
                relaunched.draft == "Newer draft\n\nOriginal submission",
                "Explicit recovery must append without overwriting newer typing")
            relaunched.dismissRecovery(saved.id, screen: relaunched.screenToken)
            try check(relaunched.recoveries.isEmpty, "Explicit dismiss must remove only the saved receipt")
            try check(
                bridge.operations.filter { $0.0 == "agent.prompt" }.count == 1,
                "Recovery actions and relaunch never resend")
            relaunched.transport(host: "", connected: false)
        }
    }

    static func eventsBeforeReceiptAndReset() async throws {
        let (model, bridge, defaults, suite) = try await setup()
        defer { model.transport(host: "", connected: false); defaults.removePersistentDomain(forName: suite) }
        try await open(model, bridge)
        model.receive(try event(#"{"type":"terminal.output","streamId":"s","data":"old screen"}"#))
        model.receive(try event(#"{"type":"terminal.size","streamId":"s","cols":132,"rows":40}"#))
        try finishOpen(bridge)
        try await wait { model.streamId != nil }
        try check(
            model.drainTerminal().compactMap(\.text) == ["old screen"], "Initial output before receipt must survive")
        try check(model.cols == 132 && model.rows == 40, "PTY size must arrive before or after receipt")
        model.receive(try event(#"{"type":"terminal.output","streamId":"s","data":"before resize"}"#))
        model.receive(try event(#"{"type":"terminal.size","streamId":"s","cols":80,"rows":24}"#))
        model.receive(try event(#"{"type":"terminal.output","streamId":"s","data":"after resize"}"#))
        let ordered = model.drainTerminal()
        try check(ordered.count == 3, "Size events must be queued between output chunks")
        if case .size(80, 24) = ordered[1] {
        } else {
            throw CheckFailure(errorDescription: "Output and actual PTY resize order was lost")
        }

        model.receive(try event(#"{"type":"terminal.output","streamId":"s","data":"stale bytes"}"#))
        let epoch = model.terminalEpoch
        model.receive(try event(#"{"type":"terminal.reset","streamId":"s","reason":"gap"}"#))
        model.receive(try event(#"{"type":"terminal.output","streamId":"s","data":"snapshot"}"#))
        try check(
            model.terminalEpoch != epoch && model.drainTerminal().compactMap(\.text) == ["snapshot"],
            "Reset must discard old pending bytes and screen/history")
        model.receive(try event(#"{"type":"terminal.output","streamId":"other","data":"foreign"}"#))
        try check(model.drainTerminal().isEmpty, "Foreign stream must not reach viewer")
    }

    static func hostSwitchFencesLateOpenAndDraftCallbacks() async throws {
        let (model, bridge, defaults, suite) = try await setup()
        defer { model.transport(host: "", connected: false); defaults.removePersistentDomain(forName: suite) }
        try await open(model, bridge)
        let oldScreen = model.screenToken
        model.editDraft("Host A draft", screen: oldScreen)
        model.receive(try event(#"{"type":"terminal.output","streamId":"s","data":"old host"}"#))
        model.transport(host: "host-b", connected: true, request: bridge.request)
        try finishOpen(bridge)
        try await wait { bridge.operations.contains { $0.0 == "terminal.close" } }
        try check(
            model.selected == nil && model.streamId == nil && model.drainTerminal().isEmpty,
            "Late open must not reopen old host surface")
        try await wait { !model.machines.isEmpty }
        try await open(model, bridge); try finishOpen(bridge, id: "new")
        try await wait { model.streamId == "new" }
        model.editDraft("New host", screen: model.screenToken)
        model.editDraft("Stale edit", screen: oldScreen)
        try check(model.draft == "New host", "Old text binding must never overwrite another agent's draft")
        model.sendKey(.interrupt, screen: oldScreen)
        try check(!bridge.operations.contains { $0.0 == "terminal.keys" }, "Old key action must never target new agent")
    }

    static func downgradeAndLateReceipt() async throws {
        let (model, bridge, defaults, suite) = try await setup()
        defer { model.transport(host: "", connected: false); defaults.removePersistentDomain(forName: suite) }
        try await open(model, bridge); try finishOpen(bridge)
        try await wait { model.streamId != nil }
        let token = model.screenToken
        model.editDraft("Keep original", screen: token); model.sendPrompt(screen: token)
        try await wait { !bridge.prompts.isEmpty }
        model.rawTyping = true
        model.receive(try event(#"{"type":"permissions","terminals":"view"}"#))
        try check(
            !model.controlEnabled && !model.canSend && !model.rawTyping,
            "Downgrade must disable every input immediately")
        bridge.prompts.removeFirst().resume(returning: try json(#"{"outcome":"confirmed","reason":null}"#))
        try await wait { model.composer.pending == nil }
        try check(
            model.draft == "Keep original" && model.composer.outcome == .failed,
            "Late receipt after downgrade must preserve uncertain draft")
        model.receive(try event(#"{"type":"permissions","terminals":"off"}"#))
        try check(
            model.selected == nil && model.machines.isEmpty && model.streamId == nil,
            "Off must close and clear terminal surfaces")
    }

    static func hostSwitchFencesLatePromptReceipt() async throws {
        let (model, bridge, defaults, suite) = try await setup()
        defer { model.transport(host: "", connected: false); defaults.removePersistentDomain(forName: suite) }
        try await open(model, bridge); try finishOpen(bridge)
        try await wait { model.streamId != nil }
        model.editDraft("Origin", screen: model.screenToken); model.sendPrompt(screen: model.screenToken)
        try await wait { !bridge.prompts.isEmpty }
        model.transport(host: "host-b", connected: true, request: bridge.request)
        try await wait { !model.machines.isEmpty }
        try await open(model, bridge); try finishOpen(bridge, id: "b")
        try await wait { model.streamId == "b" }
        model.editDraft("New host draft", screen: model.screenToken)
        bridge.prompts.removeFirst().resume(returning: try json(#"{"outcome":"confirmed","reason":null}"#))
        for _ in 0..<20 { await Task.yield() }
        try check(
            model.draft == "New host draft" && model.composer.outcome == nil,
            "Originating receipt must not modify current host")
        model.transport(host: "host-a", connected: true, request: bridge.request)
        try await wait { !model.machines.isEmpty }
        try await open(model, bridge); try finishOpen(bridge, id: "a2")
        try await wait { model.streamId == "a2" }
        try check(
            model.draft == "Origin" && model.composer.outcome == .failed,
            "Uncertain original draft must remain recoverable on return")
    }
}

extension AgentControlScenarios {
    static func heldInventoryCoalescesAndMaximumPromptKeepsNewDraft() async throws {
        let (model, bridge, defaults, suite) = try await setup()
        defer { model.transport(host: "", connected: false); defaults.removePersistentDomain(forName: suite) }
        try await open(model, bridge); try finishOpen(bridge)
        try await wait { model.streamId != nil }
        bridge.holdInventory = true
        let held = Task { await model.refresh() }
        try await wait { bridge.inventories.count == 1 }
        for _ in 0..<40 { model.sendKey(.up, screen: model.screenToken) }
        for _ in 0..<20 { await model.refresh() }
        try await Task.sleep(nanoseconds: 350_000_000)
        try check(bridge.inventories.count == 1, "A held inventory must coalesce key and explicit refresh bursts")
        bridge.holdInventory = false
        bridge.inventories.removeFirst().resume(returning: try json(bridge.inventory)); _ = await held.value
        let original = String(repeating: "a", count: 65536)
        model.editDraft(original, screen: model.screenToken); model.sendPrompt(screen: model.screenToken)
        try await wait { bridge.prompts.count == 1 }
        try check(bridge.operations.last(where: { $0.0 == "agent.prompt" })?.1["text"] as? String == original, "Maximum prompt must be delivered intact")
        model.editDraft("Newer draft", screen: model.screenToken)
        bridge.prompts.removeFirst().resume(returning: try json(#"{"outcome":"failed","reason":"Delivery uncertain"}"#))
        try await wait { model.composer.pending == nil }
        try check(model.draft == "Newer draft" && model.recoveries.contains(where: { $0.text == original }), "Uncertain maximum submission and newer draft must both survive")
    }
}

extension AgentControlScenarios {
    static func workspaceFencesAndOfflinePins() async throws {
        let (model, bridge, defaults, suite) = try await setup()
        defer { model.transport(host: "", connected: false); defaults.removePersistentDomain(forName: suite) }
        let scope = model.shortcuts[0].scope
        model.togglePin(scope)
        bridge.inventory = #"{"machines":[{"id":"local","name":"Mac","online":false,"screenMayBeInexact":false,"complete":false,"error":"Offline","agents":[]}]}"#
        await model.refresh()
        try check(model.pinnedShortcuts.count == 1 && !model.pinnedShortcuts[0].machine.online, "Offline inventory must retain unavailable pinned context")
        try model.receive(event(#"{"type":"permissions","terminals":"off","workspaceManagement":false}"#))
        try check(model.pinnedShortcuts.isEmpty, "Off must clear restricted cached shortcuts")
        try model.receive(event(#"{"type":"permissions","terminals":"control","workspaceManagement":true,"workspace":{"management":true}}"#))
        for _ in 0..<30 { await Task.yield() }
        bridge.holdInventory = true
        let operation = Task { try await model.workspaceRequest("workspace.rename", fields: ["operationId":"operation-one"]) }
        try await wait { bridge.inventories.count == 1 }
        try model.receive(event(#"{"type":"permissions","terminals":"view","workspaceManagement":false}"#))
        bridge.holdInventory = false; bridge.inventories.removeFirst().resume(returning: try json(bridge.inventory))
        do { _ = try await operation.value; throw CheckFailure(errorDescription: "Late management result must be fenced after refresh downgrade") }
        catch is AgentControlFailure {}
        let status = Task { try await model.operationStatus("operation-one") }
        try await wait { bridge.managementStatuses.count == 1 }
        try model.receive(event(#"{"type":"permissions","terminals":"off"}"#))
        bridge.managementStatuses.removeFirst().resume(returning: try json(#"{"operationId":"operation-one","state":"succeeded","message":"restricted"}"#))
        do { _ = try await status.value; throw CheckFailure(errorDescription: "Late status must be fenced by read permission revision") }
        catch is AgentControlFailure {}
    }
}

extension AgentControlScenarios {
    static func visibleInventoryFreshnessIsSingleFlightAndPauses() async throws {
        let (model, bridge, defaults, suite) = try await setup(interval: 20_000_000)
        defer { model.setInventoryVisible(false); model.transport(host: "", connected: false); defaults.removePersistentDomain(forName: suite) }
        bridge.holdInventory = true
        model.setInventoryVisible(true)
        try await wait { bridge.inventories.count == 1 }
        for _ in 0..<20 { await model.refresh() }
        try await Task.sleep(nanoseconds: 80_000_000)
        try check(bridge.inventories.count == 1, "Slow visible inventory must have only one request in flight")
        bridge.holdInventory = false
        let count = bridge.operations.filter { $0.0 == "agents" }.count
        bridge.inventories.removeFirst().resume(returning: try json(bridge.inventory))
        try await Task.sleep(nanoseconds: 80_000_000)
        try check(bridge.operations.filter { $0.0 == "agents" }.count > count, "Visible inventory must refresh periodically")
        model.setInventoryVisible(false)
        let stopped = bridge.operations.filter { $0.0 == "agents" }.count
        try await Task.sleep(nanoseconds: 80_000_000)
        try check(bridge.operations.filter { $0.0 == "agents" }.count == stopped, "Pi-only surfaces must pause inventory reads")
        model.setInventoryVisible(true); model.setInventoryForeground(false)
        let background = bridge.operations.filter { $0.0 == "agents" }.count
        try await Task.sleep(nanoseconds: 80_000_000)
        try check(bridge.operations.filter { $0.0 == "agents" }.count == background, "Background must pause visible inventory")
    }
}

extension AgentControlScenarios {
    static func thrownPromptReceiptIsUncertainAndKeepsBothDrafts() async throws {
        let (model, bridge, defaults, suite) = try await setup()
        defer { model.transport(host: "", connected: false); defaults.removePersistentDomain(forName: suite) }
        try await open(model, bridge); try finishOpen(bridge)
        try await wait { model.streamId != nil }
        model.editDraft("Submitted original", screen: model.screenToken); model.sendPrompt(screen: model.screenToken)
        try await wait { bridge.prompts.count == 1 }
        model.editDraft("Newer draft", screen: model.screenToken)
        bridge.prompts.removeFirst().resume(throwing: AgentControlFailure(message: "No prompt acknowledgement from your Mac"))
        try await wait { model.composer.pending == nil }
        try check(model.composer.deliveryUncertain && model.draft == "Newer draft", "No acknowledgement must show uncertainty without replacing newer typing")
        try check(model.recoveries.contains(where: { $0.text == "Submitted original" && $0.status == "Delivery uncertain" }), "Original must retain explicit uncertain recovery label")
        model.closeScreen()
        let relaunched = AgentControlModel(defaults: defaults)
        relaunched.transport(host: "host-a", connected: true, request: bridge.request)
        try await wait { !relaunched.machines.isEmpty }; try await open(relaunched, bridge); try finishOpen(bridge)
        try await wait { relaunched.streamId != nil }
        try check(relaunched.draft == "Newer draft" && relaunched.recoveries.contains(where: { $0.status == "Delivery uncertain" }), "Both drafts and uncertain label must survive relaunch")
        try check(bridge.operations.filter { $0.0 == "agent.prompt" }.count == 1, "Timeout must never replay")
        relaunched.transport(host: "", connected: false)
    }
}

extension AgentControlScenarios {
    static func authoritativeReviewRequiresSuccessfulFreshInventory() async throws {
        let (model, bridge, defaults, suite) = try await setup()
        defer { model.transport(host: "", connected: false); defaults.removePersistentDomain(forName: suite) }
        let missing = await model.refreshWorkspaceForReview(machine: "local", project: "/repo")
        try check(!missing, "Legacy or incomplete metadata must not count as authoritative review")
        bridge.inventory = #"{"machines":[{"id":"local","name":"Mac","online":true,"screenMayBeInexact":false,"complete":true,"metadataStatus":"complete","agents":[],"projects":[{"id":"/repo","name":"Repo","available":true}],"worktrees":[]}]}"#
        let success = await model.refreshWorkspaceForReview(machine: "local", project: "/repo")
        try check(success, "Current complete target inventory permits deliberate separate operation")
        bridge.holdInventory = true
        let held = Task { await model.refresh() }
        try await wait { bridge.inventories.count == 1 }
        let skipped = await model.refreshWorkspaceForReview(machine: "local", project: "/repo")
        try check(!skipped, "A skipped inflight refresh must never count as review")
        bridge.inventories.removeFirst().resume(throwing: AgentControlFailure(message: "Offline")); _ = await held.value
        bridge.holdInventory = false
        bridge.inventory = #"{"machines":[{"id":"local","name":"Mac","online":false,"screenMayBeInexact":false,"complete":false,"metadataStatus":"offline","agents":[],"projects":[],"worktrees":[]}]}"#
        let offline = await model.refreshWorkspaceForReview(machine: "local", project: "/repo")
        try check(!offline, "Offline cached metadata must never count as review")
        try model.receive(event(#"{"type":"permissions","terminals":"off"}"#))
        let off = await model.refreshWorkspaceForReview(machine: "local", project: "/repo")
        try check(!off, "Permission off must never count as review")
    }
}

extension AgentControlScenarios {
    static func duplicateInventoryKeepsFirstScopedIdentity() async throws {
        let (model, bridge, defaults, suite) = try await setup()
        defer { model.transport(host: "", connected: false); defaults.removePersistentDomain(forName: suite) }
        let original = bridge.inventory
        var root = try JSONSerialization.jsonObject(with: Data(original.utf8)) as! [String: Any]
        var machine = (root["machines"] as! [[String: Any]])[0]
        let agent = (machine["agents"] as! [[String: Any]])[0]
        var conflictingAgent = agent; conflictingAgent["name"] = "Wrong later identity"
        machine["agents"] = [agent, conflictingAgent]
        let project: [String: Any] = ["id":"/repo", "name":"First project", "available":true]
        machine["projects"] = [project, project]
        let worktree: [String: Any] = ["projectId":"/repo", "id":"w", "name":"Worktree", "path":"/repo/w", "branch":"feature", "status":"active", "agentIds":["a", "a"]]
        machine["worktrees"] = [worktree, worktree]
        var later = machine; later["name"] = "Wrong later machine"
        var otherMachine = machine; otherMachine["id"] = "other"; otherMachine["name"] = "Other scoped machine"
        root["machines"] = [machine, later, otherMachine]
        bridge.inventory = String(decoding: try JSONSerialization.data(withJSONObject: root), as: UTF8.self)
        await model.refresh()
        try check(model.machines.count == 2, "Duplicate registry machine IDs must produce one first consistent machine")
        let first = model.machines.first { $0.id == "local" }!
        try check(first.name == "Mac" && first.agents.count == 1 && first.agents[0].name == "Agent", "Duplicate agent IDs must preserve first identity within each machine")
        try check(first.registeredProjects?.count == 1 && first.worktrees?.count == 1, "Project and worktree ForEach identities must be unique")
        try check(model.shortcuts.count == 2 && Set(model.shortcuts.map(\.id)).count == 2, "Cross-machine same agent IDs remain distinct without duplicate shortcut rows")
        let scope = model.shortcuts.first { $0.scope.machine == "local" }!.scope
        model.togglePin(scope)
        try check(model.pinnedShortcuts.count == 1 && model.recentShortcuts.count == 1, "Pin lookup must not trap or duplicate rows")
        let corruptPins = try JSONSerialization.data(withJSONObject: ["pins":[scope.storageKey,scope.storageKey],"interactions":[:],"hidden":[]])
        defaults.set(corruptPins, forKey: "pi.agentNavigation.v1")
        let restored = AgentControlModel(defaults: defaults)
        restored.transport(host: "host-a", connected: true, request: bridge.request)
        try await wait { !restored.machines.isEmpty }
        try check(restored.pinnedShortcuts.count == 1 && Set(restored.pinnedShortcuts.map(\.id)).count == 1, "Duplicate persisted pin keys cannot duplicate SwiftUI rows")
        restored.transport(host: "", connected: false)
    }
}

extension AgentControlScenarios {
    static func workspaceGrantToggleDoesNotInvalidatePrompt() async throws {
        let (model, bridge, defaults, suite) = try await setup()
        defer { model.transport(host: "", connected: false); defaults.removePersistentDomain(forName: suite) }
        try await open(model, bridge); try finishOpen(bridge)
        try await wait { model.streamId != nil }
        let screen = model.screenToken, stream = model.streamId
        model.editDraft("Keep valid prompt confirmation", screen: screen); model.sendPrompt(screen: screen)
        try await wait { bridge.prompts.count == 1 }
        try model.receive(event(#"{"type":"permissions","terminals":"control","workspaceManagement":false,"workspace":{"management":true}}"#))
        try model.receive(event(#"{"type":"permissions","terminals":"control","workspaceManagement":true,"workspace":{"management":true}}"#))
        try check(model.streamId == stream && model.screenToken == screen, "Workspace-only grant changes must retain terminal attachment")
        bridge.prompts.removeFirst().resume(returning: try json(#"{"outcome":"confirmed"}"#))
        try await wait { model.composer.pending == nil }
        try check(model.composer.outcome == .confirmed && !model.composer.deliveryUncertain && model.draft.isEmpty, "Workspace-only toggle must not invalidate authoritative terminal confirmation")
        try check(model.recoveries.isEmpty && bridge.operations.filter { $0.0 == "agent.prompt" }.count == 1, "Confirmed prompt must not retain uncertainty or replay")
        try await wait { !model.loading }
        bridge.holdInventory = true
        let mutation = Task { try await model.workspaceRequest("workspace.rename", fields: ["operationId":"workspace-toggle-check"]) }
        try await wait { bridge.inventories.count == 1 }
        try model.receive(event(#"{"type":"permissions","terminals":"control","workspaceManagement":false,"workspace":{"management":true}}"#))
        try model.receive(event(#"{"type":"permissions","terminals":"control","workspaceManagement":true,"workspace":{"management":true}}"#))
        bridge.holdInventory = false
        bridge.inventories.removeFirst().resume(returning: try json(bridge.inventory))
        do { _ = try await mutation.value; throw CheckFailure(errorDescription: "Management revoke and regrant must still fence the old mutation") }
        catch is AgentControlFailure {}
        try check(model.streamId == stream && model.composer.outcome == .confirmed, "Management fence must leave confirmed terminal state intact")
    }
}
