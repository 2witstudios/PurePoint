import SwiftUI

struct SidebarView: View {
    @Binding var selection: SidebarSelection?
    @Environment(AppState.self) private var appState
    @Environment(GridState.self) private var gridState

    var body: some View {
        VStack(spacing: 0) {
            // Nav items — fixed, non-scrollable
            VStack(spacing: 2) {
                ForEach(SidebarNavItem.allCases) { item in
                    SidebarNavButton(
                        item: item,
                        isSelected: selection == .nav(item)
                    ) {
                        selection = .nav(item)
                    }
                }
            }
            .padding(.horizontal, 8)
            .padding(.vertical, 8)

            Divider()
                .padding(.horizontal, 8)

            // Project tree — NSOutlineView for compact 24pt rows
            if appState.isLoaded {
                SidebarOutlineView(
                    selection: $selection,
                    appState: appState,
                    gridState: gridState
                )
                .padding(.top, 8)
            } else {
                VStack {
                    Spacer()
                    Text("No project open")
                        .font(PurePointTheme.smallFont)
                        .foregroundStyle(.tertiary)
                    Spacer()
                }
            }
        }
        .safeAreaInset(edge: .bottom) {
            SidebarFooter(selection: selection)
        }
    }
}

// MARK: - SidebarNavButton

private struct SidebarNavButton: View {
    let item: SidebarNavItem
    let isSelected: Bool
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            Label(item.title, systemImage: item.icon)
                .font(PurePointTheme.navFont)
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(.horizontal, 8)
                .padding(.vertical, 4)
                .background(
                    isSelected
                        ? AnyShapeStyle(.selection)
                        : AnyShapeStyle(.clear)
                )
                .clipShape(RoundedRectangle(cornerRadius: 5))
        }
        .buttonStyle(.plain)
    }
}
