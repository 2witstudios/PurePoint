import SwiftUI

/// Recursively renders a PaneSplitNode tree as nested draggable split views.
struct PaneGridView: View {
    @Environment(GridState.self) private var gridState

    var body: some View {
        rootView(gridState.root)
    }

    /// Always returns AnyView(DraggableSplit<AnyView, AnyView>) so the
    /// wrapped type never changes when root transitions between .split
    /// and .leaf (e.g. 2->1 pane close), preventing SwiftUI from destroying
    /// and recreating the entire view hierarchy.
    private func rootView(_ node: PaneSplitNode) -> AnyView {
        switch node {
        case .leaf(let id, let agentId):
            return AnyView(
                DraggableSplit(
                    axis: .vertical,
                    ratio: 1.0,
                    onRatioChanged: { _ in }
                ) {
                    AnyView(
                        PaneCellView(
                            leafId: id,
                            agentId: agentId,
                            isFocused: id == gridState.focusedLeafId
                        )
                    )
                } second: {
                    AnyView(Color.clear)
                }
            )
        case .split:
            return nodeView(node)
        }
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
            let splitId = first.firstLeafId
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
