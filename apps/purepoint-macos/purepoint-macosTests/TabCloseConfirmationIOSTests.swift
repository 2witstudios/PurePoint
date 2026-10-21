#if os(iOS)
import Foundation
import Testing
import UIKit

@testable import PurePoint

/// Closing an iPad tab that would lose something asks the Mac's question, and its close
/// button closes the tab, instead of refusing outright.
@MainActor
struct TabCloseConfirmationIOSTests {
    @Test func aTabWithUnsavedChangesOffersTheMacsChoice() {
        let alert = TabCloseConfirmation.alert(
            closing: 1, reasons: ["Unsaved changes to main.rs will be lost."], close: {})
        #expect(alert.title == "Close this tab?")
        #expect(alert.message == "Unsaved changes to main.rs will be lost.")
        #expect(alert.actions.map(\.title) == ["Close Tab", "Cancel"])
        #expect(alert.actions.map(\.style) == [.destructive, .cancel])
    }

    @Test func aPaneAsksAboutThePane() {
        let alert = TabCloseConfirmation.alert(
            closing: 2, reasons: ["2 running agents will be stopped."], close: {})
        #expect(alert.title == "Close this pane?")
        #expect(alert.actions.map(\.title) == ["Close Pane", "Cancel"])
    }

    @Test func closingThatLosesNothingClosesAtOnce() {
        let registry = WorkspaceRegistry()
        registry.reconcile(
            projectRoot: NSTemporaryDirectory() + "pp-tab-close-" + UUID().uuidString,
            rootAgents: [AgentModel(id: "ag-a", name: "ag-a", agentType: "claude", status: .running, prompt: "", startedAt: "")],
            worktrees: [])
        let tabs = registry.workspace(id: "ws-ag-a")!.panes[0]!.tabs
        let appState = AppState(startProject: { _ in })
        #expect(TabCloseConfirmation.reasons(closing: tabs, in: "ws-ag-a", appState: appState, registry: registry).isEmpty)
        var closed = false
        TabCloseConfirmation.close(tabs, in: "ws-ag-a", appState: appState, registry: registry) { closed = true }
        #expect(closed)
    }
}
#endif
