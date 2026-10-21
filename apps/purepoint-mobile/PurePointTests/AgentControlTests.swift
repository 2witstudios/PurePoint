import XCTest
import UIKit
import SwiftTerm
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
    @MainActor func testGivenTransportDropShouldReopenSameAgentOnlyOnSameHostUnlessClosed() async throws {
        try await AgentControlScenarios.reconnectReopensDroppedAgentOnlyOnSameHost()
    }
    @MainActor func testGivenDroppedAgentMissingAfterReconnectShouldStayClosed() async throws {
        try await AgentControlScenarios.reconnectSkipsDroppedAgentMissingFromInventory()
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

extension AgentControlTests {
    @MainActor func testGivenDuplicateInventoryShouldKeepFirstScopedIdentityWithoutDuplicateRows() async throws {
        try await AgentControlScenarios.duplicateInventoryKeepsFirstScopedIdentity()
    }
    @MainActor func testGivenWorkspaceOnlyGrantToggleShouldPreserveTerminalPromptConfirmation() async throws {
        try await AgentControlScenarios.workspaceGrantToggleDoesNotInvalidatePrompt()
    }
}

extension AgentControlTests {
    func testGivenMissingScopedAgentShouldShowIdentityFallbackWithoutPointGuardStatus() {
        let context = AgentDestinationContext.resolve(scope: AgentScope(host: "bridge", machine: "remote", agent: "worker"), machines: [], connected: true)
        XCTAssertEqual(context.agentName, "worker")
        XCTAssertEqual(context.machineName, "remote")
        XCTAssertEqual(context.projectLabel, "Project unavailable")
        XCTAssertEqual(context.worktreeLabel, "Location unavailable")
        XCTAssertTrue(context.status.contains("Unavailable"))
    }
}

extension AgentControlTests {
    func testGivenMaximumFleetScheduleShouldKeepAgentsDeadlineBeyondEveryBoundedBatch() {
        let maximumMachines = 32, concurrentMachines = 4
        let batches = (maximumMachines + concurrentMachines - 1) / concurrentMachines
        let seconds = batches * (5 + 5 + max(5, 5))
        XCTAssertEqual(seconds, 120)
        XCTAssertEqual(MobileRPCTiming.timeoutNanoseconds(for: "agents"), UInt64(seconds + 30) * 1_000_000_000)
        XCTAssertEqual(MobileRPCTiming.timeoutNanoseconds(for: "agent.prompt"), 390_000_000_000)
        XCTAssertEqual(MobileRPCTiming.timeoutNanoseconds(for: "sync"), 35_000_000_000)
    }
}

extension AgentControlTests {
    @MainActor func testGivenHeldWorkspaceLaunchShouldKeepReceiptAndDurableUncertainFormWithoutReplay() async throws {
        try await AgentControlScenarios.heldWorkspaceSpawnReceiptPreservesDurableRecoveryAndNeverReplays()
    }
    func testGivenWorkspaceOperationsShouldUseOnlyTheirExplicitReceiptBudgets() {
        XCTAssertEqual(MobileRPCTiming.timeoutNanoseconds(for: "workspace.spawn"), 390_000_000_000)
        for operation in ["workspace.terminal", "workspace.createWorktree", "workspace.rename", "workspace.stop"] {
            XCTAssertEqual(MobileRPCTiming.timeoutNanoseconds(for: operation), 100_000_000_000)
        }
        for operation in ["workspace.catalog", "workspace.operation", "workspace.unrecognized", "sync"] {
            XCTAssertEqual(MobileRPCTiming.timeoutNanoseconds(for: operation), 35_000_000_000)
        }
    }
}

@MainActor private final class TerminalGesturePan: UIPanGestureRecognizer {
    var touchCount = 1
    var phase: UIGestureRecognizer.State = .changed
    var movement = CGPoint.zero
    override var numberOfTouches: Int { touchCount }
    override var state: UIGestureRecognizer.State {
        get { phase }
        set { phase = newValue }
    }
    override func translation(in view: UIView?) -> CGPoint { movement }
    override func setTranslation(_ translation: CGPoint, in view: UIView?) { movement = translation }
}

@MainActor private final class TerminalGesturePinch: UIPinchGestureRecognizer {
    var touchCount = 0
    var phase: UIGestureRecognizer.State = .possible
    override var numberOfTouches: Int { touchCount }
    override var state: UIGestureRecognizer.State {
        get { phase }
        set { phase = newValue }
    }
}

@MainActor private final class TerminalGestureScroll: UIScrollView {
    let testPinch = TerminalGesturePinch()
    override var pinchGestureRecognizer: UIPinchGestureRecognizer? { testPinch }
}

extension AgentControlTests {
    @MainActor func testGivenWheelPanAlreadyRecognizedShouldStillAllowViewportPinch() {
        let coordinator = AgentTerminalView.Coordinator()
        let scroll = TerminalGestureScroll()
        coordinator.scroll = scroll
        let pan = TerminalGesturePan()
        XCTAssertTrue(coordinator.gestureRecognizer(pan, shouldRecognizeSimultaneouslyWith: scroll.testPinch))
        XCTAssertFalse(coordinator.gestureRecognizer(pan, shouldRecognizeSimultaneouslyWith: scroll.panGestureRecognizer))
        XCTAssertFalse(coordinator.gestureRecognizer(pan, shouldRecognizeSimultaneouslyWith: UIPinchGestureRecognizer()))
    }

    @MainActor func testGivenSecondTouchAfterWheelPanBeganShouldStopWheelForwardingDuringPinch() {
        let coordinator = AgentTerminalView.Coordinator()
        let scroll = TerminalGestureScroll()
        coordinator.scroll = scroll
        let pan = TerminalGesturePan()
        XCTAssertTrue(coordinator.canForwardScroll(pan))
        // The pan may still report one tracked touch after a second finger arrives.
        scroll.testPinch.touchCount = 2
        XCTAssertFalse(coordinator.canForwardScroll(pan))
        scroll.testPinch.touchCount = 1
        scroll.testPinch.phase = .changed
        XCTAssertFalse(coordinator.canForwardScroll(pan))
        scroll.testPinch.phase = .ended
        pan.touchCount = 2
        XCTAssertFalse(coordinator.canForwardScroll(pan))
        pan.touchCount = 0
        XCTAssertFalse(coordinator.canForwardScroll(pan))
        pan.touchCount = 1
        XCTAssertTrue(coordinator.canForwardScroll(pan))
    }
}


extension AgentControlTests {
    @MainActor func testGivenPinchDuringWheelPanShouldDiscardTranslationBeforePanResumes() {
        let coordinator = AgentTerminalView.Coordinator()
        let scroll = TerminalGestureScroll()
        coordinator.scroll = scroll
        let pan = TerminalGesturePan()
        scroll.testPinch.touchCount = 2
        pan.movement = CGPoint(x: 24, y: 120)
        coordinator.scrollAgent(pan)
        XCTAssertEqual(pan.movement, .zero)
        // A continuing pinch can still be active after its touch count drops.
        scroll.testPinch.touchCount = 1
        scroll.testPinch.phase = .changed
        pan.movement = CGPoint(x: 6, y: 60)
        coordinator.scrollAgent(pan)
        XCTAssertEqual(pan.movement, .zero)
        scroll.testPinch.phase = .ended
        pan.movement = CGPoint(x: 0, y: 12)
        XCTAssertTrue(coordinator.canForwardScroll(pan))
        XCTAssertEqual(pan.translation(in: nil).y, 12, "Only subsequent pan movement remains")
    }
}

extension AgentControlTests {
    @MainActor func testGivenAgentEnablesMouseModeShouldNotAddCompetingTerminalPan() {
        let terminal = ReadOnlyAgentTerminal(frame: CGRect(x: 0, y: 0, width: 800, height: 480))
        terminal.feed(text: "\u{1b}[?1049h\u{1b}[?1003h\u{1b}[?1006h")
        XCTAssertTrue(terminal.getTerminal().isCurrentBufferAlternate)
        XCTAssertNotEqual(terminal.getTerminal().mouseMode, .off)
        let pans = (terminal.gestureRecognizers ?? []).filter { $0 is UIPanGestureRecognizer }
        XCTAssertEqual(pans.count, 1)
        XCTAssertTrue(pans.first === terminal.panGestureRecognizer, "Only the view's own scroll pan remains")
    }

    @MainActor func testGivenReadOnlyTerminalShouldNotOfferSwiftTermSelectionGestures() {
        let terminal = ReadOnlyAgentTerminal(frame: CGRect(x: 0, y: 0, width: 800, height: 480))
        terminal.disableBuiltInSelection()
        let selecting = (terminal.gestureRecognizers ?? []).filter {
            ($0 is UITapGestureRecognizer || $0 is UILongPressGestureRecognizer) && $0.isEnabled
        }
        XCTAssertTrue(selecting.isEmpty, "Taps belong to the viewport's fit-width and keyboard gestures")
    }
}
