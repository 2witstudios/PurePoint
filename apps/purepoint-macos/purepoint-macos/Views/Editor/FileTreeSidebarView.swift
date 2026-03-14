import SwiftUI

struct FileTreeSidebarView: View {
    var fileTreeState: FileTreeState
    @Binding var showFileTree: Bool
    var onFileSelected: (String, String) -> Void

    var body: some View {
        VStack(spacing: 0) {
            // Header
            HStack {
                Button {
                    withAnimation(.easeInOut(duration: 0.2)) {
                        showFileTree = false
                    }
                } label: {
                    Image(systemName: "sidebar.left")
                }
                .buttonStyle(.plain)
                .help("Hide file tree")

                Text("Files")
                    .font(.system(size: 12, weight: .semibold))

                Spacer()
            }
            .padding(.horizontal, 12)
            .padding(.top, 8)
            .padding(.bottom, 4)

            // Search
            TextField("Search files...", text: Bindable(fileTreeState).searchQuery)
                .textFieldStyle(.roundedBorder)
                .padding(.horizontal, 12)
                .padding(.vertical, 8)

            Divider()

            // File tree
            FileTreeOutlineView(
                fileTreeState: fileTreeState,
                onFileSelected: onFileSelected
            )
        }
    }
}
