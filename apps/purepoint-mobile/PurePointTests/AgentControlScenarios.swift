import Foundation
#if !PHONE_CONTROL_CHECKS
    @testable import PurePoint
#endif

@MainActor enum AgentControlScenarios {
    @MainActor final class Bridge {
        var opens: [CheckedContinuation<JSONValue, Error>] = []
        var prompts: [CheckedContinuation<JSONValue, Error>] = []
        var operations: [(String, [String: Any])] = []
        var inventory = ""
        func request(_ op: String, _ fields: [String: Any]) async throws -> JSONValue {
            operations.append((op, fields))
            switch op {
            case "permissions": return .object(["terminals": .string("control")])
            case "agents": return try JSONDecoder().decode(JSONValue.self, from: Data(inventory.utf8))
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
    static func setup() async throws -> (AgentControlModel, Bridge, UserDefaults, String) {
        let suite = "phone-agent-checks-" + UUID().uuidString
        let defaults = UserDefaults(suiteName: suite)!
        let model = AgentControlModel(defaults: defaults), bridge = Bridge()
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
