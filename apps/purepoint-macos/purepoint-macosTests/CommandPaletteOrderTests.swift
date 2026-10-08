import Foundation
import Testing
@testable import PurePoint

@MainActor
struct CommandPaletteOrderTests {
    private func items(order: [String] = []) -> [CommandPaletteItem] {
        CommandPaletteItem.buildItems(
            builtInVariants: AgentVariant.variantsWithWorktree,
            agents: [
                AgentDefinition(name: "custom", command: "my-command"),
                AgentDefinition(name: "hidden", availableInCommandDialog: false),
            ],
            swarms: [],
            preferredOrder: order
        )
    }

    @Test func defaultOrderPreservesExistingEntries() {
        #expect(items().map(\.displayName) == ["Claude", "Codex", "OpenCode", "Terminal", "Worktree", "custom"])
    }

    @Test func customCommandCanPrecedeBuiltIns() {
        let order = ["agentdef:local:custom", "builtin:codex:agent", "builtin:claude:agent"]
        #expect(
            items(order: order).map(\.displayName) == ["custom", "Codex", "Claude", "OpenCode", "Terminal", "Worktree"])
    }

    @Test func missingAndDuplicatePreferencesDoNotDropEntries() {
        let order = ["missing", "builtin:codex:agent", "builtin:codex:agent", "builtin:claude:worktree"]
        #expect(
            items(order: order).map(\.displayName) == ["Codex", "Worktree", "Claude", "OpenCode", "Terminal", "custom"])
    }

    @Test func tabPaletteKeepsOrderWhenWorktreeIsUnavailable() {
        let palette = CommandPaletteItem.buildItems(
            builtInVariants: AgentVariant.allVariants, agents: [], swarms: [], includeFiles: true,
            preferredOrder: ["builtin:claude:worktree", "builtin:opencode:agent", "builtin:codex:agent"]
        )
        #expect(palette.map(\.displayName) == ["OpenCode", "Codex", "Claude", "Terminal", "Files"])
    }

    @Test func movingEntriesPersistsAcrossSettingsInstances() throws {
        let suite = "CommandPaletteOrderTests.\(UUID().uuidString)"
        let defaults = try #require(UserDefaults(suiteName: suite))
        defer { defaults.removePersistentDomain(forName: suite) }
        let settings = SettingsState(defaults: defaults)

        settings.moveCommandPaletteItem("builtin:codex:agent", by: -1, items: items())
        let restored = SettingsState(defaults: defaults)
        #expect(items(order: restored.commandPaletteOrder).first?.displayName == "Codex")

        let unchanged = restored.commandPaletteOrder
        restored.moveCommandPaletteItem("builtin:codex:agent", by: -1, items: items(order: unchanged))
        #expect(restored.commandPaletteOrder == unchanged)

        restored.commandPaletteOrder = []
        #expect(SettingsState(defaults: defaults).commandPaletteOrder.isEmpty)
    }

    @Test func movingPreservesUnavailablePreferences() throws {
        let suite = "CommandPaletteOrderTests.\(UUID().uuidString)"
        let defaults = try #require(UserDefaults(suiteName: suite))
        defer { defaults.removePersistentDomain(forName: suite) }
        let settings = SettingsState(defaults: defaults)
        settings.commandPaletteOrder = ["unavailable", "agentdef:local:custom", "builtin:claude:agent"]
        let visible = items(order: settings.commandPaletteOrder)
        settings.moveCommandPaletteItem("agentdef:local:custom", by: 1, items: visible)
        #expect(settings.commandPaletteOrder.first == "unavailable")
        #expect(items(order: settings.commandPaletteOrder).prefix(2).map(\.displayName) == ["Claude", "custom"])
    }
}
