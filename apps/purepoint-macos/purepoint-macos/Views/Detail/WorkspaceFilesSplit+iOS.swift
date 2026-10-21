#if os(iOS)
import SwiftUI

/// The Mac workspace layout's `HSplitView` on iPad: the content, a hairline divider, then the
/// files sidebar. The divider drags the sidebar's width between the Mac's frame limits
/// (240–640pt, starting at its 360pt ideal, never squeezing the content under 160pt), and a
/// window resize goes to the content, which has the higher layout priority on the Mac.
struct WorkspaceFilesSplit<Content: View, Sidebar: View>: View {
    let showSidebar: Bool
    @ViewBuilder let content: () -> Content
    @ViewBuilder let sidebar: () -> Sidebar

    static var minSidebarWidth: CGFloat { 240 }
    static var idealSidebarWidth: CGFloat { 360 }
    static var maxSidebarWidth: CGFloat { 640 }
    static var minContentWidth: CGFloat { 160 }
    /// The hit area each side of the hairline, as AppKit's split view divider.
    private static var dragInset: CGFloat { 4 }

    @State private var sidebarWidth: CGFloat = 360  // `idealSidebarWidth`
    /// The width under the finger while dragging, committed when the drag ends.
    @State private var dragWidth: CGFloat?
    @State private var dragStartWidth: CGFloat?

    /// The width the sidebar gets in a split `total` wide.
    static func clampedSidebarWidth(_ width: CGFloat, total: CGFloat) -> CGFloat {
        let bounded = min(max(width, minSidebarWidth), maxSidebarWidth)
        return max(0, min(bounded, total - minContentWidth - 1))
    }

    var body: some View {
        GeometryReader { geo in
            let width = Self.clampedSidebarWidth(dragWidth ?? sidebarWidth, total: geo.size.width)
            HStack(spacing: 0) {
                content()
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
                if showSidebar {
                    divider(total: geo.size.width)
                    sidebar()
                        .frame(width: width)
                        .frame(maxHeight: .infinity)
                }
            }
        }
    }

    private func divider(total: CGFloat) -> some View {
        Rectangle()
            .fill(Color(nsColor: .separatorColor))
            .frame(width: 1)
            .frame(maxHeight: .infinity)
            .padding(.horizontal, Self.dragInset)
            .contentShape(Rectangle())
            .padding(.horizontal, -Self.dragInset)
            .hoverEffect(.highlight)
            .gesture(
                DragGesture(minimumDistance: 1, coordinateSpace: .global)
                    .onChanged { value in
                        let start = dragStartWidth ?? sidebarWidth
                        dragStartWidth = start
                        // Dragging left widens the sidebar.
                        dragWidth = Self.clampedSidebarWidth(start - value.translation.width, total: total)
                    }
                    .onEnded { _ in
                        if let dragWidth { sidebarWidth = dragWidth }
                        dragWidth = nil
                        dragStartWidth = nil
                    }
            )
            .accessibilityLabel("Resize files sidebar")
    }
}
#endif
