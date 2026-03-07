import SwiftUI

struct SidebarView: View {
    @Binding var selection: SidebarSelection?
    @Environment(AppState.self) private var appState
    @Environment(GridState.self) private var gridState
    @Environment(TerminalViewCache.self) private var viewCache
    var onOutlineViewReady: ((NSOutlineView) -> Void)?

    var body: some View {
        VStack(spacing: 0) {
            // App branding — doubles as Dashboard button
            Button {
                selection = .nav(.dashboard)
            } label: {
                HStack(spacing: 6) {
                    Image("PurePointLogo")
                        .resizable()
                        .aspectRatio(contentMode: .fit)
                        .frame(width: 16, height: 16)
                    Text("PurePoint")
                        .font(.system(size: 13, weight: .semibold))
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(.horizontal, 8)
                .padding(.vertical, 4)
                .background(
                    selection == .nav(.dashboard)
                        ? AnyShapeStyle(.selection)
                        : AnyShapeStyle(.clear)
                )
                .clipShape(RoundedRectangle(cornerRadius: 5))
            }
            .buttonStyle(.plain)
            .padding(.horizontal, 8)
            .padding(.top, 8)

            // Nav items — fixed, non-scrollable
            VStack(spacing: 2) {
                ForEach(SidebarNavItem.allCases.filter { $0 != .dashboard }) { item in
                    SidebarNavButton(
                        item: item,
                        isSelected: selection == .nav(item)
                    ) {
                        selection = .nav(item)
                    }
                }
            }
            .padding(.horizontal, 8)
            .padding(.vertical, 4)

            Divider()
                .padding(.horizontal, 8)

            // Project tree — NSOutlineView for compact 24pt rows
            if appState.isLoaded {
                SidebarOutlineView(
                    selection: $selection,
                    appState: appState,
                    gridState: gridState,
                    viewCache: viewCache,
                    onOutlineViewReady: onOutlineViewReady
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
