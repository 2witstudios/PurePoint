import SwiftUI
import AppKit

/// Reserved for future use — replaces GeometryReader in PaneGridView
/// when user-draggable dividers are needed.
///
/// AppKit bridge for NSSplitView with programmatic ratio control.
/// SwiftUI's HSplitView/VSplitView don't support setting ratios.
struct NSSplitViewRepresentable: NSViewRepresentable {
    let axis: PaneSplitNode.Axis
    let ratio: CGFloat
    let first: NSView
    let second: NSView

    func makeNSView(context: Context) -> NSSplitView {
        let splitView = NSSplitView()
        splitView.isVertical = (axis == .vertical)
        splitView.dividerStyle = .thin
        splitView.delegate = context.coordinator

        splitView.addSubview(first)
        splitView.addSubview(second)

        return splitView
    }

    func updateNSView(_ splitView: NSSplitView, context: Context) {
        context.coordinator.ratio = ratio
        splitView.adjustSubviews()
    }

    func makeCoordinator() -> Coordinator {
        Coordinator(ratio: ratio)
    }

    @MainActor
    class Coordinator: NSObject, NSSplitViewDelegate {
        var ratio: CGFloat

        init(ratio: CGFloat) {
            self.ratio = ratio
        }

        func splitView(_ splitView: NSSplitView, constrainSplitPosition proposedPosition: CGFloat, ofSubviewAt dividerIndex: Int) -> CGFloat {
            let total = splitView.isVertical ? splitView.bounds.width : splitView.bounds.height
            let dividerThickness = splitView.dividerThickness
            return total * ratio - dividerThickness / 2
        }

        func splitView(_ splitView: NSSplitView, resizeSubviewsWithOldSize oldSize: NSSize) {
            guard splitView.subviews.count >= 2 else {
                splitView.adjustSubviews()
                return
            }

            let total = splitView.isVertical ? splitView.bounds.width : splitView.bounds.height
            let dividerThickness = splitView.dividerThickness
            let firstSize = total * ratio - dividerThickness / 2
            let secondSize = total - firstSize - dividerThickness

            var firstFrame = splitView.subviews[0].frame
            var secondFrame = splitView.subviews[1].frame

            if splitView.isVertical {
                firstFrame.size.width = firstSize
                firstFrame.size.height = splitView.bounds.height
                secondFrame.origin.x = firstSize + dividerThickness
                secondFrame.size.width = secondSize
                secondFrame.size.height = splitView.bounds.height
            } else {
                firstFrame.size.height = firstSize
                firstFrame.size.width = splitView.bounds.width
                secondFrame.origin.y = firstSize + dividerThickness
                secondFrame.size.height = secondSize
                secondFrame.size.width = splitView.bounds.width
            }

            splitView.subviews[0].frame = firstFrame
            splitView.subviews[1].frame = secondFrame
        }
    }
}
