#if os(iOS)
import Foundation
import Testing
import UIKit

@testable import PurePoint

/// IPAD-6.8: the palette overlay matches the Mac panel's size, filtering and toggling.
@MainActor
@Suite(.serialized) struct CommandPalettePanelIOSTests {

    private var items: [CommandPaletteItem] {
        CommandPaletteItem.buildItems(builtInVariants: AgentVariant.variantsWithWorktree, agents: [], swarms: [])
    }

    @Test func panelHeightFollowsTheMacFormula() {
        let header: CGFloat = 57, row: CGFloat = 44, padding: CGFloat = 8
        #expect(CommandPaletteViewController.selectionPanelHeight(rowCount: 0) == header + row + padding)
        #expect(CommandPaletteViewController.selectionPanelHeight(rowCount: 3) == header + 3 * row + padding)
        #expect(CommandPaletteViewController.selectionPanelHeight(rowCount: 50) == CGFloat(380))
    }

    @Test func emptyQueryShowsEverythingButFiles() {
        let file = CommandPaletteItem.file(PaletteFileEntry(relativePath: "Sources/EditorState.swift", absolutePath: "/r/Sources/EditorState.swift"))
        let filtered = CommandPaletteViewController.filter(items + [file], query: "  ")
        #expect(filtered.map(\.id) == items.map(\.id))
    }

    @Test func nameMatchesRankAboveTextMatches() {
        let filtered = CommandPaletteViewController.filter(items, query: "term")
        #expect(filtered.first?.displayName == "Terminal")
    }

    @Test func filesMatchBySubsequence() {
        let file = CommandPaletteItem.file(PaletteFileEntry(relativePath: "Sources/EditorState.swift", absolutePath: "/r/Sources/EditorState.swift"))
        #expect(CommandPaletteViewController.filter(items + [file], query: "edst").map(\.id) == [file.id])
    }

    /// Presentation and dismissal finish on later run loop turns.
    private func waitUntilDismissed(_ panel: CommandPalettePanel) async throws {
        for _ in 0..<100 where panel.presentingViewController != nil {
            try await Task.sleep(for: .milliseconds(10))
        }
    }

    private func keyWindow() throws -> UIWindow {
        try #require(NSApp.keyWindow)
    }

    @Test func showsCenteredAtTheMacWidthAndToggles() async throws {
        let window = try keyWindow()
        var selected = false
        let panel = CommandPalettePanel.show(relativeTo: window, items: items) { _ in selected = true }
        defer { panel.dismiss() }
        #expect(panel.presentingViewController != nil)

        panel.view.layoutIfNeeded()
        let content = panel.paletteViewController.view!
        let frame = content.convert(content.bounds, to: panel.view)
        #expect(frame.width == CommandPalettePanel.panelWidth)
        #expect(abs(frame.midX - panel.view.bounds.midX) < 0.5)
        #expect(frame.height == CommandPaletteViewController.selectionPanelHeight(rowCount: items.count))
        #expect(abs(frame.minY - max(panel.view.bounds.midY - CommandPalettePanel.topOffsetAboveCenter, 8 + panel.view.safeAreaInsets.top)) < 0.5)

        // The same shortcut again closes it, as on the Mac.
        let again = CommandPalettePanel.show(relativeTo: window, items: items) { _ in }
        #expect(again === panel)
        try await waitUntilDismissed(panel)
        #expect(panel.presentingViewController == nil)
        #expect(!selected)
    }

    @Test func presenterHookOpensThePalette() async throws {
        _ = try keyWindow()
        CommandPalettePanel.registerPresenter()
        let hook = try #require(AppLifecycleHooks.presentPalette)
        hook(CommandPalettePresentation(items: items, usePointGuardNames: true) { _ in })

        let root = try #require(NSApp.keyWindow?.rootViewController)
        var top = root
        while let presented = top.presentedViewController { top = presented }
        let panel = try #require(top as? CommandPalettePanel)
        panel.dismiss()
        try await waitUntilDismissed(panel)
    }

    @Test func escapeFromTheListDismisses() async throws {
        let window = try keyWindow()
        let panel = CommandPalettePanel.show(relativeTo: window, items: items) { _ in }
        panel.paletteViewController.handleEscape()
        try await waitUntilDismissed(panel)
        #expect(panel.presentingViewController == nil)
    }
}
#endif
