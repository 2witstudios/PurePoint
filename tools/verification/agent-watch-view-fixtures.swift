// Dependency fixtures for isolated native component typechecking. This is not a whole-app build.
import SwiftUI
import Observation
@Observable @MainActor final class ProjectState {
    var projectRoot = "/tmp/project"
    var projectName = "purepoint"
    var allAgents: [AgentModel] = []
    var rootAgents: [AgentModel] = []
    var worktrees: [WorktreeModel] = []
    let channel = ChannelState(projectRoot: "/tmp/project")
}
enum SidebarSelection: Hashable { case channel(String); case worktree(String) }
struct GHUnavailableView: View { var body: some View { EmptyView() } }
nonisolated enum PaneSplitNode { enum Axis { case vertical, horizontal } }
@MainActor final class SyntaxHighlightManager {
    init(textView: NSTextView) {}
    func setLanguage(_ language: EditorLanguage) {}
    func invalidate() {}
}
