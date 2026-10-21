#if os(iOS)
import SwiftUI
import UIKit

/// The Mac workspace layout's `HSplitView` on iPad: the content, a hairline divider, then the
/// files sidebar. The divider drags the sidebar's width between the Mac's frame limits
/// (240–640pt, never squeezing the content under 160pt), and a window resize goes to the
/// content, which has the higher layout priority on the Mac. The width is remembered, as the
/// Mac split view autosaves its divider.
struct WorkspaceFilesSplit<Content: View, Sidebar: View>: View {
    let showSidebar: Bool
    @ViewBuilder let content: () -> Content
    @ViewBuilder let sidebar: () -> Sidebar

    static var minSidebarWidth: CGFloat { 240 }
    /// Where a new iPad starts: the sidebar's minimum plus room for the tab header, narrower than
    /// the Mac's 360pt ideal because an iPad window is narrower than a Mac's.
    static var defaultSidebarWidth: CGFloat { 280 }
    static var maxSidebarWidth: CGFloat { 640 }
    static var minContentWidth: CGFloat { 160 }
    /// The hit area each side of the hairline. Wider than AppKit's divider: a finger or the
    /// iPad pointer needs more than the Mac cursor.
    static var dragInset: CGFloat { 8 }

    @AppStorage("workspaceFilesSidebarWidth") private var sidebarWidth = Double(defaultSidebarWidth)
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
            let width = Self.clampedSidebarWidth(dragWidth ?? CGFloat(sidebarWidth), total: geo.size.width)
            HStack(spacing: 0) {
                content()
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
                if showSidebar {
                    Rectangle()
                        .fill(dragWidth == nil ? Color(nsColor: .separatorColor) : Color.accentColor)
                        .frame(width: 1)
                        .frame(maxHeight: .infinity)
                    sidebar()
                        .frame(width: width)
                        .frame(maxHeight: .infinity)
                }
            }
            // Above both columns, so a terminal or list under the hairline never takes the drag.
            .overlay(alignment: .trailing) {
                if showSidebar {
                    handle(total: geo.size.width)
                        .offset(x: Self.dragInset - width - 0.5)
                }
            }
        }
    }

    private func handle(total: CGFloat) -> some View {
        SplitDividerPointerArea()
            .frame(width: Self.dragInset * 2)
            .frame(maxHeight: .infinity)
            .contentShape(Rectangle())
            .gesture(
                DragGesture(minimumDistance: 1, coordinateSpace: .global)
                    .onChanged { value in
                        let start = dragStartWidth ?? Self.clampedSidebarWidth(CGFloat(sidebarWidth), total: total)
                        dragStartWidth = start
                        // Dragging left widens the sidebar.
                        dragWidth = Self.clampedSidebarWidth(start - value.translation.width, total: total)
                    }
                    .onEnded { _ in
                        if let dragWidth { sidebarWidth = Double(dragWidth) }
                        dragWidth = nil
                        dragStartWidth = nil
                    }
            )
            .accessibilityElement()
            .accessibilityLabel("Resize files sidebar")
            .accessibilityValue("\(Int(Self.clampedSidebarWidth(CGFloat(sidebarWidth), total: total))) points")
            .accessibilityAdjustableAction { direction in
                let current = Self.clampedSidebarWidth(CGFloat(sidebarWidth), total: total)
                let step: CGFloat = direction == .increment ? 40 : -40
                sidebarWidth = Double(Self.clampedSidebarWidth(current + step, total: total))
            }
    }
}

/// The divider's hit area: transparent, and the iPad pointer becomes a vertical bar over it, as
/// over the system split view's divider (SwiftUI's `columnResize` pointer is macOS-only).
private struct SplitDividerPointerArea: UIViewRepresentable {
    func makeUIView(context: Context) -> UIView {
        let view = UIView()
        view.backgroundColor = .clear
        view.addInteraction(UIPointerInteraction(delegate: context.coordinator))
        return view
    }

    func updateUIView(_ uiView: UIView, context: Context) {}

    func makeCoordinator() -> Coordinator { Coordinator() }

    final class Coordinator: NSObject, UIPointerInteractionDelegate {
        func pointerInteraction(_ interaction: UIPointerInteraction, styleFor region: UIPointerRegion) -> UIPointerStyle? {
            UIPointerStyle(shape: .roundedRect(CGRect(x: 0, y: 0, width: 4, height: 28), radius: 2))
        }
    }
}
#endif
