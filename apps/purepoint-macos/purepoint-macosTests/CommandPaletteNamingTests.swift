import AppKit
import Foundation
import Testing
@testable import PurePoint

@MainActor
struct CommandPaletteNamingTests {
    private func descendants(_ view: NSView) -> [NSView] {
        [view] + view.subviews.flatMap { descendants($0) }
    }

    private func controls(_ vc: CommandPaletteViewController) throws -> (NSTextField, NSTextView) {
        let views = descendants(vc.view)
        let name = try #require(views.compactMap { $0 as? NSTextField }.first { $0.isEditable && $0.isBezeled })
        let prompt = try #require(views.compactMap { $0 as? NSTextView }.first)
        return (name, prompt)
    }

    private func enterName(_ vc: CommandPaletteViewController, _ field: NSTextField) {
        _ = vc.control(field, textView: NSTextView(), doCommandBy: #selector(NSResponder.insertNewline(_:)))
    }

    @Test func preferenceDefaultsOnAndPersistsOff() throws {
        let suite = "CommandPaletteNamingTests.\(UUID().uuidString)"
        let defaults = try #require(UserDefaults(suiteName: suite))
        defer { defaults.removePersistentDomain(forName: suite) }
        let settings = SettingsState(defaults: defaults)
        #expect(settings.usePointGuardNames)
        settings.usePointGuardNames = false
        #expect(!SettingsState(defaults: defaults).usePointGuardNames)
        settings.usePointGuardNames = true
        #expect(SettingsState(defaults: defaults).usePointGuardNames)
    }

    @Test func individualLaunchesRequireNameBeforePromptAndPreserveDrafts() throws {
        let items: [CommandPaletteItem] = AgentVariant.allVariants.map { .builtIn($0) }
            + [.agentDef(AgentDefinition(name: "custom", command: "my-command"))]
        for item in items {
            let vc = CommandPaletteViewController()
            vc.usePointGuardNames = false
            let (name, prompt) = try controls(vc)
            var results: [CommandPaletteResult] = []
            vc.onSelect = { results.append($0) }
            vc.selectItem(item)
            #expect(!name.isHidden)
            #expect(prompt.enclosingScrollView?.isHidden == true)
            name.stringValue = " \n "
            enterName(vc, name)
            #expect(results.isEmpty)
            #expect(!name.isHidden)
            name.stringValue = "  Fix Auth  "
            enterName(vc, name)
            #expect(name.isHidden)
            #expect(prompt.enclosingScrollView?.isHidden == false)
            prompt.string = "Investigate login"
            vc.handleEscape()
            #expect(!name.isHidden)
            #expect(name.stringValue == "Fix Auth")
            vc.handleEscape()
            #expect(results.isEmpty)
            vc.selectItem(item)
            #expect(name.stringValue == "Fix Auth")
            enterName(vc, name)
            #expect(prompt.string == "Investigate login")
            _ = vc.textView(prompt, doCommandBy: #selector(NSResponder.insertNewline(_:)))
            #expect(results.count == 1)
            switch try #require(results.first) {
            case .spawnBuiltIn(_, let submittedPrompt, let submittedName),
                 .spawnAgentDef(_, let submittedPrompt, let submittedName):
                #expect(submittedPrompt == "Investigate login")
                #expect(submittedName == "Fix Auth")
            default: Issue.record("Expected individual launch")
            }
        }
    }

    @Test func inlinePromptRequiresNameAndThenLaunchesWithoutPromptStep() throws {
        let vc = CommandPaletteViewController()
        vc.usePointGuardNames = false
        let (name, prompt) = try controls(vc)
        var results: [CommandPaletteResult] = []
        vc.onSelect = { results.append($0) }
        vc.selectItem(.agentDef(AgentDefinition(name: "review", inlinePrompt: "Review changes")))
        #expect(results.isEmpty)
        name.stringValue = "Review Auth"
        enterName(vc, name)
        #expect(prompt.enclosingScrollView?.isHidden == true)
        guard case .spawnAgentDef(let def, let submittedPrompt, let submittedName) = try #require(results.first) else {
            Issue.record("Expected custom agent launch")
            return
        }
        #expect(def.inlinePrompt == "Review changes")
        #expect(submittedPrompt == nil)
        #expect(submittedName == "Review Auth")
    }

    @Test func automaticNamingAndNonAgentActionsKeepExistingFlow() throws {
        let vc = CommandPaletteViewController()
        let (name, prompt) = try controls(vc)
        var results: [CommandPaletteResult] = []
        var dismissals = 0
        vc.onSelect = { results.append($0) }
        vc.onDismiss = { dismissals += 1 }
        vc.selectItem(.builtIn(.codex))
        #expect(name.isHidden)
        _ = vc.textView(prompt, doCommandBy: #selector(NSResponder.insertNewline(_:)))
        guard case .spawnBuiltIn(_, _, let submittedName) = try #require(results.first) else {
            Issue.record("Expected built-in launch")
            return
        }
        #expect(submittedName == nil)
        results.removeAll()
        vc.usePointGuardNames = false
        vc.selectItem(.builtIn(.worktree))
        #expect(!name.isHidden)
        #expect(prompt.enclosingScrollView?.isHidden == true)
        vc.handleEscape()
        vc.handleEscape()
        #expect(dismissals == 1)
        #expect(results.isEmpty)
        vc.selectItem(.files)
        vc.selectItem(.swarm(SwarmDefinition(name: "team")))
        #expect(results.count == 2)
    }
}
