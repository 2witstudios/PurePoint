#if os(iOS)
import Foundation
import Testing
import UIKit

@testable import PurePoint

/// IPAD-6.13: several windows, each with its own shell, and Focus Content without `NSView` walks.
@MainActor
@Suite struct IPadWindowingTests {

    @Test func infoPlistEnablesMultipleScenes() throws {
        let manifest = try #require(
            Bundle.main.object(forInfoDictionaryKey: "UIApplicationSceneManifest") as? [String: Any])
        #expect(manifest["UIApplicationSupportsMultipleScenes"] as? Bool == true)
    }

    @Test func focusOrderPutsTheFocusedPanesAgentFirst() {
        var workspace = Workspace.adopting(agentId: "A", container: .projectRoot)
        let right = workspace.split(leafId: 0, axis: .vertical, content: .agent("B"))
        workspace.newTab(leafId: 0, content: .agent("C"))
        workspace.split(leafId: right ?? 1, axis: .horizontal, content: .agent("D"))
        workspace.focusedLeafId = right ?? 1

        #expect(TerminalViewCache.focusOrder(of: workspace) == ["B", "A", "C", "D"])
    }

    @Test func focusOrderFallsBackToLayoutOrderWithoutAFocusedAgent() {
        var workspace = Workspace.adopting(agentId: "A", container: .projectRoot)
        let right = workspace.split(leafId: 0, axis: .vertical, content: .empty)
        workspace.focusedLeafId = right ?? 1

        #expect(TerminalViewCache.focusOrder(of: workspace) == ["A"])
    }

    @Test func noTerminalToFocusWithoutAWorkspace() {
        #expect(TerminalViewCache().terminalToFocus(in: nil, window: UIWindow()) == nil)
    }
}
#endif
