import SwiftUI

/// Pure SwiftUI draggable split view with programmatic ratio control.
/// Nests recursively for the pane grid tree.
///
/// Both children get exact sizes and are clipped, so a child that can't shrink to its share
/// never paints over the divider or a neighbouring pane. The drag handle sits in an overlay
/// above both children, so a terminal beside the hairline never takes the drag.
struct DraggableSplit<First: View, Second: View>: View {
    let axis: PaneSplitNode.Axis
    let ratio: CGFloat
    let onRatioChanged: (CGFloat) -> Void
    @ViewBuilder let first: () -> First
    @ViewBuilder let second: () -> Second

    static var dividerThickness: CGFloat { 1 }
    /// The hit area each side of the hairline: a finger or the iPad pointer needs more than the
    /// hairline itself.
    static var dragInset: CGFloat { 8 }
    /// The smallest a pane can be dragged to.
    static var minPaneSize: CGFloat { 120 }

    /// Live ratio while dragging. Committed through `onRatioChanged` only when
    /// the drag ends: each commit rewrites the workspace registry, which every
    /// pane, the sidebar and the menus observe.
    @State private var dragRatio: CGFloat?
    @State private var dragStartRatio: CGFloat?

    /// The two children's sizes along the split axis in a split `total` long. A ratio of 0 or 1
    /// collapses one side and drops the divider.
    static func sizes(total: CGFloat, ratio: CGFloat) -> (first: CGFloat, second: CGFloat) {
        let total = max(total, 0)
        if ratio <= 0 { return (0, total) }
        if ratio >= 1 { return (total, 0) }
        let available = max(total - dividerThickness, 0)
        let first = (available * ratio).rounded()
        return (first, available - first)
    }

    /// The ratio after dragging `translation` from `start`, keeping each side at least
    /// `minPaneSize` when the split has room for both.
    static func ratio(start: CGFloat, translation: CGFloat, total: CGFloat) -> CGFloat {
        guard total > 0 else { return 0.5 }
        let proposed = start + translation / total
        let minRatio = minPaneSize / total
        guard minRatio < 0.5 else { return 0.5 }
        return min(max(proposed, minRatio), 1 - minRatio)
    }

    var body: some View {
        let ratio = dragRatio ?? self.ratio
        GeometryReader { geo in
            let isVertical = axis == .vertical
            let total = isVertical ? geo.size.width : geo.size.height
            let collapsed = ratio >= 1.0 || ratio <= 0.0
            let sizes = Self.sizes(total: total, ratio: ratio)

            Group {
                if isVertical {
                    HStack(spacing: 0) {
                        first()
                            .frame(width: sizes.first)
                            .frame(maxHeight: .infinity)
                            .clipped()
                        if !collapsed { hairline }
                        second()
                            .frame(width: sizes.second)
                            .frame(maxHeight: .infinity)
                            .clipped()
                    }
                } else {
                    VStack(spacing: 0) {
                        first()
                            .frame(height: sizes.first)
                            .frame(maxWidth: .infinity)
                            .clipped()
                        if !collapsed { hairline }
                        second()
                            .frame(height: sizes.second)
                            .frame(maxWidth: .infinity)
                            .clipped()
                    }
                }
            }
            .frame(width: geo.size.width, height: geo.size.height, alignment: .topLeading)
            .overlay(alignment: .topLeading) {
                if !collapsed {
                    let offset = sizes.first + Self.dividerThickness / 2 - Self.dragInset
                    handle(total: total, isVertical: isVertical)
                        .offset(x: isVertical ? offset : 0, y: isVertical ? 0 : offset)
                }
            }
        }
    }

    private var hairline: some View {
        Rectangle()
            .fill(dragRatio == nil ? Color(nsColor: .separatorColor) : Color.accentColor)
            .frame(
                width: axis == .vertical ? Self.dividerThickness : nil,
                height: axis == .vertical ? nil : Self.dividerThickness
            )
    }

    private func handle(total: CGFloat, isVertical: Bool) -> some View {
        handleArea(isVertical: isVertical)
            .frame(
                width: isVertical ? Self.dragInset * 2 : nil,
                height: isVertical ? nil : Self.dragInset * 2
            )
            .frame(
                maxWidth: isVertical ? nil : .infinity,
                maxHeight: isVertical ? .infinity : nil
            )
            .contentShape(Rectangle())
            .gesture(
                DragGesture(minimumDistance: 1, coordinateSpace: .global)
                    .onChanged { value in
                        let start = dragStartRatio ?? self.ratio
                        dragStartRatio = start
                        let translation = isVertical ? value.translation.width : value.translation.height
                        dragRatio = Self.ratio(start: start, translation: translation, total: total)
                    }
                    .onEnded { _ in
                        if let dragRatio {
                            onRatioChanged(dragRatio)
                        }
                        dragRatio = nil
                        dragStartRatio = nil
                    }
            )
            .accessibilityElement()
            .accessibilityLabel("Resize pane")
            .accessibilityValue("\(Int((self.ratio * 100).rounded())) percent")
            .accessibilityAdjustableAction { direction in
                let step: CGFloat = direction == .increment ? 0.05 : -0.05
                onRatioChanged(Self.ratio(start: self.ratio, translation: step * total, total: total))
            }
    }

    @ViewBuilder
    private func handleArea(isVertical: Bool) -> some View {
        #if os(macOS)
        Color.clear
            .onHover { hovering in
                if hovering {
                    if isVertical {
                        NSCursor.resizeLeftRight.push()
                    } else {
                        NSCursor.resizeUpDown.push()
                    }
                } else {
                    NSCursor.pop()
                }
            }
        #else
        SplitDividerPointerArea(axis: axis)
        #endif
    }
}
