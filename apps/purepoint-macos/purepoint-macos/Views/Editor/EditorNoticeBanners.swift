import SwiftUI

/// Notices an editor needs whatever view hosts it: a file that could not be opened, a remote
/// file opened read-only, and a remote save refused because the file changed on the machine.
struct EditorNoticeBanners: View {
    let editor: EditorState
    /// Reports the result of an overwrite to the hosting view's save-error banner.
    var onSaveError: (String?) -> Void = { _ in }

    var body: some View {
        VStack(spacing: 0) {
            if let error = editor.openError {
                notice(error, icon: "exclamationmark.triangle.fill", color: .orange) {
                    Button("Dismiss") { editor.dismissOpenError() }
                }
            }
            if let reason = editor.readOnlyReason {
                notice(reason, icon: "lock.fill", color: .secondary) { EmptyView() }
            }
            if editor.saveConflict, let file = editor.currentFile {
                notice(
                    "\(file.name) changed on the remote machine since it was opened.",
                    icon: "exclamationmark.triangle.fill", color: .yellow
                ) {
                    Button("Reload") { editor.reloadFile() }
                    Button("Overwrite") {
                        Task {
                            do {
                                try await editor.overwriteRemoteChanges()
                                onSaveError(nil)
                            } catch {
                                onSaveError("Failed to save \(file.name): \(error.localizedDescription)")
                            }
                        }
                    }
                }
            }
        }
    }

    private func notice<Actions: View>(
        _ text: String, icon: String, color: Color, @ViewBuilder actions: () -> Actions
    ) -> some View {
        HStack(spacing: 8) {
            Image(systemName: icon).foregroundStyle(color).accessibilityHidden(true)
            Text(text).font(.caption).lineLimit(2)
            Spacer()
            actions().controlSize(.small)
        }
        .padding(.horizontal, 10).padding(.vertical, 4)
        .background(color.opacity(0.15))
        .accessibilityElement(children: .contain)
    }
}
