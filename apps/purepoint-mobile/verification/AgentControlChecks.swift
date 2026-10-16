import Foundation
@main struct AgentControlChecks {
    @MainActor static func main() async throws {
        for outcome in [AgentPromptOutcome.confirmed, .sent, .blocked, .failed] {
            var c = AgentComposer(text: "Original")
            let first = c.begin()!; c.finish(first, outcome: outcome, reason: nil)
            precondition(c.text == (outcome == .confirmed || outcome == .sent ? "" : "Original"))
            c.text = "Next"; let next = c.begin()!; c.text = "New typing"
            c.finish(next, outcome: outcome, reason: nil); precondition(c.text == "New typing")
        }
        precondition(
            AgentQuickKey.allCases.map(\.bytes) == [
                [27], [3], [9], [13], [27, 91, 65], [27, 91, 66], [27, 91, 68], [27, 91, 67], [121], [110],
            ])
        try await AgentControlScenarios.allPromptOutcomesAndNoResend()
        try await AgentControlScenarios.closeUsesOriginatingTransportAndNeverLeavesPendingDraft()
        try await AgentControlScenarios.failedOriginalAndNewerDraftSurviveNavigationHostChangeAndRelaunch()
        try await AgentControlScenarios.eventsBeforeReceiptAndReset()
        try await AgentControlScenarios.hostSwitchFencesLateOpenAndDraftCallbacks()
        try await AgentControlScenarios.downgradeAndLateReceipt()
        try await AgentControlScenarios.hostSwitchFencesLatePromptReceipt()
        try await AgentControlScenarios.heldInventoryCoalescesAndMaximumPromptKeepsNewDraft()
        precondition(MobileRPCTiming.timeoutNanoseconds(for: "agent.prompt") == 390_000_000_000)
        precondition(MobileRPCTiming.timeoutNanoseconds(for: "agents") == 150_000_000_000)
        precondition(MobileRPCTiming.timeoutNanoseconds(for: "sync") == 35_000_000_000)
        try await AgentControlScenarios.workspaceFencesAndOfflinePins()
        try await AgentControlScenarios.visibleInventoryFreshnessIsSingleFlightAndPauses()
        try await AgentControlScenarios.thrownPromptReceiptIsUncertainAndKeepsBothDrafts()
        try await AgentControlScenarios.authoritativeReviewRequiresSuccessfulFreshInventory()
        try await AgentControlScenarios.workspaceGrantToggleDoesNotInvalidatePrompt()
        try await AgentControlScenarios.duplicateInventoryKeepsFirstScopedIdentity()
        print("Phone control lifecycle and domain checks passed")
    }
}
