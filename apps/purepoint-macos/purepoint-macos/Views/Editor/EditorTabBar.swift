import SwiftUI

struct EditorTabBar: View {
    var editorState: EditorState
    var onCloseTab: (String) -> Void

    var body: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: 0) {
                changesTab
                ForEach(editorState.openTabs) { tab in
                    fileTab(tab)
                }
            }
        }
        .frame(height: 32)
        .background(Color(nsColor: Theme.cardHeaderBackground))
    }

    // MARK: - Changes Tab

    private var changesTab: some View {
        let isActive = editorState.showChangesTab

        return Button {
            editorState.activateChangesTab()
        } label: {
            HStack(spacing: 4) {
                Image(systemName: "doc.badge.plus")
                    .font(.system(size: 10))
                Text("Changes")
                    .font(.system(size: 11))
            }
            .padding(.horizontal, 12)
            .frame(height: 32)
            .background(isActive ? Color(nsColor: Theme.cardBackground) : Color.clear)
            .overlay(alignment: .bottom) {
                if isActive {
                    Rectangle()
                        .fill(Color.accentColor)
                        .frame(height: 2)
                }
            }
        }
        .buttonStyle(.plain)
    }

    // MARK: - File Tab

    private func fileTab(_ tab: EditorTab) -> some View {
        FileTabButton(
            tab: tab,
            isActive: editorState.activeTabId == tab.id,
            onSelect: { editorState.setActiveTab(id: tab.id) },
            onClose: { onCloseTab(tab.id) }
        )
    }
}

private struct FileTabButton: View {
    let tab: EditorTab
    let isActive: Bool
    var onSelect: () -> Void
    var onClose: () -> Void
    @State private var isHovered = false

    var body: some View {
        HStack(spacing: 4) {
            Image(systemName: tab.language.icon)
                .font(.system(size: 10))
                .foregroundStyle(.secondary)

            Text(tab.name)
                .font(.system(size: 11))
                .lineLimit(1)

            if tab.isDirty {
                Circle()
                    .fill(Color.primary.opacity(0.4))
                    .frame(width: 6, height: 6)
            }

            if isActive || isHovered {
                Button {
                    onClose()
                } label: {
                    Image(systemName: "xmark")
                        .font(.system(size: 8, weight: .bold))
                        .foregroundStyle(.secondary)
                }
                .buttonStyle(.plain)
            }
        }
        .padding(.horizontal, 10)
        .frame(height: 32)
        .contentShape(Rectangle())
        .onTapGesture(perform: onSelect)
        .background(isActive ? Color(nsColor: Theme.cardBackground) : Color.clear)
        .overlay(alignment: .bottom) {
            if isActive {
                Rectangle()
                    .fill(Color.accentColor)
                    .frame(height: 2)
            }
        }
        .onHover { isHovered = $0 }
    }
}
