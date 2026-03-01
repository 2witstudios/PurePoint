import SwiftUI

struct NavButtonsSection: View {
    @Binding var selection: SidebarSelection?

    var body: some View {
        VStack(spacing: 2) {
            ForEach(SidebarNavItem.allCases) { item in
                NavButton(
                    item: item,
                    isSelected: selection == .nav(item),
                    action: { selection = .nav(item) }
                )
            }
        }
        .padding(.horizontal, PurePointTheme.padding)
        .padding(.vertical, 4)
    }
}

private struct NavButton: View {
    let item: SidebarNavItem
    let isSelected: Bool
    let action: () -> Void

    @State private var isHovered = false

    var body: some View {
        Button(action: action) {
            HStack(spacing: 6) {
                Image(systemName: item.icon)
                    .frame(width: 16)
                Text(item.title)
                    .font(PurePointTheme.navFont)
                Spacer()
            }
            .frame(height: PurePointTheme.navRowHeight)
            .padding(.horizontal, PurePointTheme.padding)
            .background(background, in: RoundedRectangle(cornerRadius: 6))
            .contentShape(RoundedRectangle(cornerRadius: 6))
        }
        .buttonStyle(.plain)
        .onHover { isHovered = $0 }
    }

    private var background: some ShapeStyle {
        if isSelected {
            AnyShapeStyle(PurePointTheme.selectionHighlight)
        } else if isHovered {
            AnyShapeStyle(PurePointTheme.hoverHighlight)
        } else {
            AnyShapeStyle(.clear)
        }
    }
}

#Preview {
    NavButtonsSection(selection: .constant(.nav(.dashboard)))
        .frame(width: 240)
        .preferredColorScheme(.dark)
}
