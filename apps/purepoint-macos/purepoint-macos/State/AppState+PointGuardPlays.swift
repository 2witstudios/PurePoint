import Foundation

extension AppState {
    /// Global `pu` prompts the Point Guard court can play. Listed from the home folder,
    /// `~/.pu/templates` is both the local and the global folder, so those come back "local".
    func pointGuardPrompts() async -> [PointGuardPrompt] {
        let home = NSHomeDirectory()
        if agentsHubState.prompts.isEmpty {
            await agentsHubState.loadTemplates(projectRoots: libraryProjectRoots.isEmpty ? [home] : libraryProjectRoots)
        }
        var seen = Set<String>()
        return agentsHubState.prompts
            .filter { $0.source == "global" || $0.projectRoot == home }
            .filter { seen.insert($0.name).inserted }
            .map { PointGuardPrompt(name: $0.name, body: $0.body, needsInput: !$0.variables.isEmpty) }
    }

    /// A saved prompt's body, fetched from the daemon when the list didn't carry it.
    func pointGuardPromptBody(_ name: String) async -> String? {
        let matches = { (prompt: SavedPrompt) in
            prompt.name == name && (prompt.source == "global" || prompt.projectRoot == NSHomeDirectory())
        }
        guard let prompt = agentsHubState.prompts.first(where: matches) else { return nil }
        if prompt.body.isEmpty {
            let root = prompt.projectRoot ?? libraryProjectRoots.first ?? NSHomeDirectory()
            await agentsHubState.loadPromptDetail(projectRoot: root, name: name)
        }
        let body = agentsHubState.prompts.first(where: matches)?.body ?? ""
        return body.isEmpty ? nil : body
    }
}
