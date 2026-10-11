import SwiftUI

/// A pane-local sidebar. Native split views can grow beyond a grid cell's
/// proposed width when their children have competing minimum sizes.
struct PointGuardConversationLayout<Actions: View, Sidebar: View, Content: View>: View {
    @Binding var isExpanded: Bool
    @ViewBuilder let actions: () -> Actions
    @ViewBuilder let sidebar: () -> Sidebar
    @ViewBuilder let content: () -> Content
    @State private var preferredWidth: CGFloat = 210
    @State private var resizeStart: CGFloat?

    var body: some View {
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
                                Text("Conversations").font(.system(size: 12, weight: .semibold)).lineLimit(1)
                            }
                            Spacer(minLength: 0)
                            if width >= 80 { actions() }
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
                    .padding(.vertical, 8)
                    .frame(maxWidth: .infinity, alignment: .trailing)

                    if isExpanded { sidebar() } else { Spacer(minLength: 0) }
                }
                .frame(width: width, height: geometry.size.height)
                .background(Color(nsColor: .controlBackgroundColor))
                .clipped()

                Rectangle()
                    .fill(Color(nsColor: .separatorColor))
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

                content()
                    .frame(width: max(0, available - width - dividerWidth), height: geometry.size.height)
                    .clipped()
            }
            .frame(width: available, height: geometry.size.height, alignment: .leading)
            .clipped()
        }
    }
}
