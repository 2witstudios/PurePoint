import SwiftUI

struct ChannelReferenceSelection: Identifiable {
    let id = UUID()
    let message: ChannelMessage
    let reference: ChannelReference
}
struct ChannelReferenceView: View {
    let project: ProjectState
    let selection: ChannelReferenceSelection
    @Environment(\.dismiss) private var dismiss
    @State private var diff: DiffData?
    @State private var error: String?
    @State private var loading = true
    var body: some View {
        VStack(spacing: 0) {
            HStack {
                Text(selection.reference.label ?? selection.reference.value).font(.headline).textSelection(.enabled)
                Spacer()
                Button("Done") { dismiss() }.keyboardShortcut(.cancelAction)
            }.padding(16)
            Divider()
            DiffListView(diff: diff, isLoading: loading, emptyMessage: "No file changes", error: error)
        }.frame(minWidth: 640, minHeight: 480)
        .task(id: selection.id) {
            let path = project.worktrees.first(where: { $0.id == selection.message.author.worktreeId })?.path ?? project.projectRoot
            do {
                if selection.reference.kind == "commit" {
                    diff = DiffData(files: try await GitService.shared.fetchCommitDiff(at: path, sha: selection.reference.value))
                } else if let number = Int(selection.reference.value) {
                    diff = try await GitService.shared.fetchPRDiffChecked(cwd: path, prNumber: number)
                } else { error = "Invalid pull request reference" }
            } catch { self.error = error.localizedDescription }
            loading = false
        }
    }
}
