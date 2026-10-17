import SwiftUI

/// A pane-local sidebar. Native split views can grow beyond a grid cell's
/// proposed width when their children have competing minimum sizes.
///
/// In split mode the rail runs the full height of the pane. Its header occupies the same
/// row as `tabBar`, so the rail and the pane's tab strip read as one band.
struct PointGuardConversationLayout<Actions: View, Sidebar: View, Content: View>: View {
    @Binding var isExpanded: Bool
    var compact = false
    /// The pane's tab strip, drawn to the right of the rail's header.
    var tabBar: AnyView = AnyView(EmptyView())
    @ViewBuilder let actions: () -> Actions
    @ViewBuilder let sidebar: () -> Sidebar
    @ViewBuilder let content: () -> Content
    @State private var preferredWidth: CGFloat = 210
    @State private var resizeStart: CGFloat?

    var body: some View {
        if compact {
            compactLayout
        } else {
            splitLayout
        }
    }

    /// History replaces chat in the workspace inspector, leaving the composer
    /// the full sidebar width when the user returns to the conversation.
    private var compactLayout: some View {
        VStack(spacing: 0) {
            HStack(spacing: 8) {
                Button {
                    isExpanded.toggle()
                } label: {
                    Image(systemName: isExpanded ? "bubble.left.and.bubble.right" : "sidebar.left")
                        .frame(width: 24, height: 24)
                }
                .help("Toggle conversations (Command-Shift-S)")
                .accessibilityLabel(isExpanded ? "Back to chat" : "Show conversations")
                Spacer(minLength: 0)
                actions()
            }
            .font(.system(size: 12))
            .buttonStyle(.borderless)
            .padding(.horizontal, 8)
            .padding(.vertical, 8)
            .foregroundStyle(.secondary)
            Group {
                if isExpanded {
                    sidebar()
                } else {
                    content()
                }
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
        }
        .frame(minWidth: 0, maxWidth: .infinity, maxHeight: .infinity)
    }

    private var splitLayout: some View {
        GeometryReader { geometry in
            let available = max(0, geometry.size.width)
            // Even a heavily compressed pane keeps enough rail for the toggle.
            let width = isExpanded ? min(preferredWidth, max(36, available * 0.45), available) : min(36, available)
            let dividerWidth = min(isExpanded ? CGFloat(4) : 1, max(0, available - width))
            let headerPadding = min(isExpanded ? CGFloat(10) : 6, max(0, (width - 24) / 2))

            HStack(spacing: 0) {
                VStack(spacing: 0) {
                    HStack(spacing: 8) {
                        if isExpanded {
                            if width >= 160 {
                                Text("CONVERSATIONS").font(.system(size: 11, weight: .semibold)).foregroundStyle(.secondary)
                                    .lineLimit(1)
                            }
                            Spacer(minLength: 0)
                        }
                        Button {
                            isExpanded.toggle()
                        } label: {
                            Image(systemName: "sidebar.left").frame(width: 24, height: 24)
                        }
                        .help("Toggle conversations (Command-Shift-S)")
                        .accessibilityLabel(isExpanded ? "Hide conversations" : "Show conversations")
                    }
                    .buttonStyle(.borderless)
                    .padding(.horizontal, headerPadding)
                    .frame(maxWidth: .infinity, alignment: .trailing)
                    // Same height and color as the tab strip, so the header joins it.
                    .frame(height: PaneTabBar.height)
                    .background(Color(nsColor: Theme.tabBarBackground))
                    .overlay(alignment: .bottom) { Divider() }
                    .environment(\.colorScheme, .dark)

                    if isExpanded { sidebar() } else { Spacer(minLength: 0) }
                }
                .frame(width: width, height: geometry.size.height)
                .background(Color(nsColor: Theme.chatRailBackground))
                .clipped()

                // The drag handle is wider than the hairline it draws.
                Rectangle()
                    .fill(Color(nsColor: Theme.chatRailBackground))
                    .overlay(alignment: .trailing) { Rectangle().fill(Color.primary.opacity(0.06)).frame(width: 1) }
                    .frame(width: dividerWidth)
                    .gesture(
                        DragGesture(minimumDistance: 1)
                            .onChanged { value in
                                guard isExpanded else { return }
                                if resizeStart == nil { resizeStart = width }
                                preferredWidth = min(280, max(180, (resizeStart ?? width) + value.translation.width))
                            }
                            .onEnded { _ in resizeStart = nil }
                    )

                VStack(spacing: 0) {
                    tabBar
                    HStack {
                        Spacer(minLength: 0)
                        actions()
                    }
                    .font(.system(size: 12))
                    .buttonStyle(.borderless)
                    .padding(.horizontal, 12)
                    .padding(.vertical, 8)
                    .foregroundStyle(.secondary)
                    content()
                        .frame(maxWidth: .infinity, maxHeight: .infinity)
                }
                    .frame(width: max(0, available - width - dividerWidth), height: geometry.size.height)
                    .clipped()
            }
            .frame(width: available, height: geometry.size.height, alignment: .leading)
            .clipped()
        }
    }
}
