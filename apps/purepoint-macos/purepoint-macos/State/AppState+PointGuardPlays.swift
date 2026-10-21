import Foundation

extension AppState {
    /// Point Guard's plays read only global `pu` prompts, so they're listed from this device's home folder: on the Mac
    /// its `.pu/templates` is the global folder itself, and a remote daemon has no such folder, so no project's local
    /// prompt can shadow a global one of the same name.
    private var pointGuardPromptRoot: String { NSHomeDirectory() }

    /// The global prompts the court can play, freshly listed; nil when the daemon can't be reached.
    func pointGuardPrompts() async -> [PointGuardPrompt]? {
        await agentsHubState.listTemplates(root: pointGuardPromptRoot)?
            .map { PointGuardPrompt(name: $0.name, body: $0.body, needsInput: !$0.variables.isEmpty) }
    }

    /// A saved prompt as it is now, body included; nil when it no longer exists.
    func pointGuardPrompt(named name: String) async -> PointGuardPrompt? {
        guard let prompt = await agentsHubState.template(root: pointGuardPromptRoot, name: name) else { return nil }
        return PointGuardPrompt(name: prompt.name, body: prompt.body, needsInput: !prompt.variables.isEmpty)
    }
}
