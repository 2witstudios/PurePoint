import SwiftUI

/// Pure SwiftUI draggable split view with programmatic ratio control.
/// Nests recursively for the pane grid tree.
struct DraggableSplit<First: View, Second: View>: View {
    let axis: PaneSplitNode.Axis
    let ratio: CGFloat
    let onRatioChanged: (CGFloat) -> Void
    @ViewBuilder let first: () -> First
    @ViewBuilder let second: () -> Second

    private let dividerThickness: CGFloat = 4

    var body: some View {
        GeometryReader { geo in
            let isVertical = axis == .vertical
            let total = isVertical ? geo.size.width : geo.size.height
            let firstSize = total * ratio - dividerThickness / 2

            if isVertical {
                HStack(spacing: 0) {
                    first().frame(width: max(firstSize, 0))
                    dividerHandle(total: total, isVertical: true)
                    second()
                }
            } else {
                VStack(spacing: 0) {
                    first().frame(height: max(firstSize, 0))
                    dividerHandle(total: total, isVertical: false)
                    second()
                }
            }
        }
    }

    private func dividerHandle(total: CGFloat, isVertical: Bool) -> some View {
        Rectangle()
            .fill(Color(nsColor: .separatorColor))
            .frame(width: isVertical ? dividerThickness : nil,
                   height: isVertical ? nil : dividerThickness)
            .contentShape(Rectangle().inset(by: -4))
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
            .gesture(DragGesture(minimumDistance: 1)
                .onChanged { value in
                    let position = isVertical ? value.location.x : value.location.y
                    let clamped = max(0.1, min(0.9, position / total))
                    onRatioChanged(clamped)
                }
            )
    }
}
