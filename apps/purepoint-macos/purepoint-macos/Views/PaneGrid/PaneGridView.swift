import SwiftUI

/// Recursively renders a PaneSplitNode tree as nested draggable split views.
struct PaneGridView: View {
    @Environment(GridState.self) private var gridState

    var body: some View {
        nodeView(gridState.root)
    }

    /// Uses AnyView to break the recursive opaque return type inference.
    private func nodeView(_ node: PaneSplitNode) -> AnyView {
        switch node {
        case .leaf(let id, let agentId):
            return AnyView(
                PaneCellView(
                    leafId: id,
                    agentId: agentId,
                    isFocused: id == gridState.focusedLeafId
                )
            )

        case .split(let axis, let ratio, let first, let second):
            let splitId = first.allLeafIds.first ?? 0
            return AnyView(
                DraggableSplit(axis: axis, ratio: ratio, onRatioChanged: { newRatio in
                    gridState.updateRatio(newRatio, forSplitIdentifiedByFirstLeaf: splitId)
                }) {
                    nodeView(first)
                } second: {
                    nodeView(second)
                }
            )
        }
    }
}
