import XCTest
@testable import PurePoint

final class AgentControlTests: XCTestCase {
    func testGivenEveryPromptOutcomeShouldKeepUnsentTextAndNeverReplaceNewerTyping() {
        for outcome in [AgentPromptOutcome.confirmed, .sent, .blocked, .failed] {
            var composer = AgentComposer(text: "Original")
            let submission = composer.begin()!
            composer.finish(submission, outcome: outcome, reason: "Reason")
            XCTAssertEqual(composer.text, outcome == .confirmed || outcome == .sent ? "" : "Original")
            XCTAssertEqual(composer.outcome, outcome)
            composer.text = "Next"
            let next = composer.begin()!
            composer.text = "New typing"
            composer.finish(next, outcome: outcome, reason: nil)
            XCTAssertEqual(composer.text, "New typing")
        }
    }

    func testGivenUnknownScreenAwarePhaseShouldBlockPromptWhileLegacyStaysUnconfirmed() throws {
        for phase in ["null", "\"unknown\""] {
            let wire =
                "{\"id\":\"a\",\"name\":\"A\",\"agentType\":\"claude\",\"state\":\"waiting\",\"screenAware\":true,\"phase\":"
                + phase + "}"
            let agent = try JSONDecoder().decode(ControlledAgent.self, from: Data(wire.utf8))
            XCTAssertNotNil(agent.promptBlockReason)
            let legacy = try JSONDecoder().decode(
                ControlledAgent.self, from: Data(wire.replacingOccurrences(of: "true", with: "false").utf8))
            XCTAssertNil(legacy.promptBlockReason)
        }
    }

    func testGivenQuickKeysShouldUseFixedBridgeNamesAndExactTerminalBytes() {
        XCTAssertEqual(
            AgentQuickKey.allCases.map(\.rawValue),
            ["Esc", "Ctrl-C", "Tab", "Enter", "ArrowUp", "ArrowDown", "ArrowLeft", "ArrowRight", "y", "n"])
        XCTAssertEqual(
            AgentQuickKey.allCases.map(\.bytes),
            [[27], [3], [9], [13], [27, 91, 65], [27, 91, 66], [27, 91, 68], [27, 91, 67], [121], [110]])
    }

    func testGivenHostMachineAndAgentScopesShouldNeverShareDrafts() throws {
        let a = AgentScope(host: "host-a", machine: "local", agent: "agent")
        XCTAssertNotEqual(a.storageKey, AgentScope(host: "host-b", machine: "local", agent: "agent").storageKey)
        XCTAssertNotEqual(a.storageKey, AgentScope(host: "host-a", machine: "remote", agent: "agent").storageKey)
        XCTAssertNotEqual(a.storageKey, AgentScope(host: "host-a", machine: "local", agent: "other").storageKey)
    }

    func testGivenWireInventoryShouldDecodeOfflineLegacyAndDialogPriority() throws {
        let data = Data(
            #"{"machines":[{"id":"local","name":"Mac","online":true,"protocolVersion":10,"screenMayBeInexact":false,"error":null,"complete":true,"agents":[{"id":"a","name":"A","agentType":"claude","state":"waiting","projectRoot":"/repo","worktreeId":null,"screenAware":true,"phase":"input_ready","idleSeconds":12,"startedAt":"2026-10-16T00:00:00Z"},{"id":"b","name":"B","agentType":"codex","state":"waiting","projectRoot":"/repo","worktreeId":null,"screenAware":true,"phase":"dialog","idleSeconds":null,"startedAt":""}]},{"id":"old","name":"Offline","online":false,"protocolVersion":null,"screenMayBeInexact":true,"error":"Unavailable","complete":false,"agents":[]}]}"#
                .utf8)
        let inventory = try JSONDecoder().decode(AgentInventory.self, from: data)
        XCTAssertEqual(inventory.machines[0].projects[0].agents.map(\.id), ["b", "a"])
        XCTAssertEqual(inventory.machines[0].agents[0].idleSeconds, 12)
        XCTAssertFalse(inventory.machines[1].online)
        XCTAssertTrue(inventory.machines[1].screenMayBeInexact)
        XCTAssertNotNil(inventory.machines[0].agents[1].promptBlockReason)
    }

    func testGivenTerminalEventsShouldDecodeResetSizeAndUnknownFields() throws {
        let reset = try JSONDecoder().decode(
            AgentTerminalEvent.self, from: Data(#"{"type":"terminal.reset","streamId":"s","reason":"gap"}"#.utf8))
        XCTAssertEqual(reset.type, "terminal.reset")
        let size = try JSONDecoder().decode(
            AgentTerminalEvent.self,
            from: Data(#"{"type":"terminal.size","streamId":"s","cols":140,"rows":42,"future":true}"#.utf8))
        XCTAssertEqual(size.cols, 140)
        XCTAssertEqual(size.rows, 42)
    }
}

extension AgentControlTests {
    @MainActor func testGivenNewerDraftShouldRetainFailedOriginalAcrossNavigationHostsAndRelaunch() async throws {
        try await AgentControlScenarios.failedOriginalAndNewerDraftSurviveNavigationHostChangeAndRelaunch()
    }

    @MainActor func testGivenScreenExitShouldKeepUncertainDraftAndCloseOnlyOriginatingTransport() async throws {
        try await AgentControlScenarios.closeUsesOriginatingTransportAndNeverLeavesPendingDraft()
    }

    @MainActor func testGivenEveryPromptResultShouldKeepDraftsAndReconnectMustNeverResend() async throws {
        try await AgentControlScenarios.allPromptOutcomesAndNoResend()
    }
    @MainActor func testGivenEventsBeforeOpenReceiptAndResetShouldRenderOnlyCurrentSnapshot() async throws {
        try await AgentControlScenarios.eventsBeforeReceiptAndReset()
    }
    @MainActor func testGivenHostSwitchShouldFenceLateOpenAndOldTextBindings() async throws {
        try await AgentControlScenarios.hostSwitchFencesLateOpenAndDraftCallbacks()
    }
    @MainActor func testGivenDowngradeShouldDisableInputImmediatelyAndPreserveUncertainDraft() async throws {
        try await AgentControlScenarios.downgradeAndLateReceipt()
    }
    @MainActor func testGivenHostSwitchShouldKeepLateReceiptInOriginatingRecoveryOnly() async throws {
        try await AgentControlScenarios.hostSwitchFencesLatePromptReceipt()
    }
}

extension AgentControlTests {
    @MainActor func testGivenHeldInventoryShouldCoalesceKeysAndPreserveMaximumUncertainPrompt() async throws {
        try await AgentControlScenarios.heldInventoryCoalescesAndMaximumPromptKeepsNewDraft()
    }
}

extension AgentControlTests {
    func testGivenScopedPinsShouldSurviveSwarmAndHideOnlyShortcut() throws {
        var preferences = AgentNavigationPreferences()
        let selected = AgentScope(host: "bridge-a", machine: "daemon", agent: "worker")
        let other = AgentScope(host: "bridge-b", machine: "daemon", agent: "worker")
        preferences.pin(selected)
        for index in 0..<30 { preferences.interact(AgentScope(host: "bridge-a", machine: "daemon", agent: "swarm\(index)")) }
        XCTAssertTrue(preferences.isPinned(selected)); XCTAssertFalse(preferences.isPinned(other))
        let restored = try JSONDecoder().decode(AgentNavigationPreferences.self, from: JSONEncoder().encode(preferences))
        XCTAssertEqual(restored, preferences)
        preferences.hide(selected); XCTAssertTrue(preferences.isHidden(selected)); XCTAssertFalse(preferences.isPinned(selected))
        preferences.interact(selected); XCTAssertFalse(preferences.isHidden(selected))
    }
    func testGivenPendingManagementFormShouldPersistOriginalOperationWithoutReplay() throws {
        var form = WorkspaceFormDraft()
        form.operationId = UUID().uuidString; form.state = "uncertain"; form.prompt = "Start this work"
        let restored = try JSONDecoder().decode(WorkspaceFormDraft.self, from: JSONEncoder().encode(form))
        XCTAssertEqual(restored, form)
        XCTAssertNotNil(restored.operationId)
    }
}

extension AgentControlTests {
    @MainActor func testGivenOfflineInventoryShouldKeepPinsAndFenceLateManagementOnDowngrade() async throws {
        try await AgentControlScenarios.workspaceFencesAndOfflinePins()
    }
}

extension AgentControlTests {
    @MainActor func testGivenVisibleSlowInventoryShouldStaySingleFlightAndPauseOffscreen() async throws {
        try await AgentControlScenarios.visibleInventoryFreshnessIsSingleFlightAndPauses()
    }
    func testGivenHistoryOpenWhenDialogArrivesShouldPresentOnlyForOriginatingSelection() {
        var overlays = MobileOverlayState()
        overlays.openHistory(); overlays.dialogArrived(selection: 17)
        XCTAssertFalse(overlays.history); XCTAssertFalse(overlays.navigator)
        XCTAssertTrue(overlays.shouldPresentDialog(selection: 17))
        XCTAssertFalse(overlays.shouldPresentDialog(selection: 18))
    }
}

extension AgentControlTests {
    @MainActor func testGivenNoPromptAcknowledgementShouldLabelUncertainAndRetainBothDrafts() async throws {
        try await AgentControlScenarios.thrownPromptReceiptIsUncertainAndKeepsBothDrafts()
    }
}

extension AgentControlTests {
    @MainActor func testGivenSkippedOfflineOrFailedRefreshShouldNotPermitSeparateLaunch() async throws {
        try await AgentControlScenarios.authoritativeReviewRequiresSuccessfulFreshInventory()
    }
}
