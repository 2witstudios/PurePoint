import SwiftUI

/// Recursively renders a PaneSplitNode tree as nested split views.
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
            return AnyView(
                GeometryReader { geo in
                    if axis == .vertical {
                        HStack(spacing: 1) {
                            nodeView(first)
                                .frame(width: geo.size.width * ratio)
                            Divider()
                            nodeView(second)
                        }
                    } else {
                        VStack(spacing: 1) {
                            nodeView(first)
                                .frame(height: geo.size.height * ratio)
                            Divider()
                            nodeView(second)
                        }
                    }
                }
            )
        }
    }
}
