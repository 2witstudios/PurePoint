import SwiftUI

/// Recursively renders a workspace's PaneSplitNode tree as nested draggable split views.
struct PaneGridView: View {
    let workspaceId: String
    @Environment(WorkspaceRegistry.self) private var registry

    var body: some View {
        if let workspace = registry.workspace(id: workspaceId) {
            rootView(workspace)
        } else {
            Color.clear
        }
    }

    /// Always returns AnyView(DraggableSplit<AnyView, AnyView>) so the
    /// wrapped type never changes when root transitions between .split
    /// and .leaf (e.g. 2->1 pane close), preventing SwiftUI from destroying
    /// and recreating the entire view hierarchy.
    private func rootView(_ workspace: Workspace) -> AnyView {
        switch workspace.root {
        case .leaf(let id):
            return AnyView(
                DraggableSplit(
                    axis: .vertical,
                    ratio: 1.0,
                    onRatioChanged: { _ in }
                ) {
                    AnyView(
                        PaneCellView(
                            workspaceId: workspaceId,
                            leafId: id,
                            isFocused: id == workspace.focusedLeafId
                        )
                    )
                } second: {
                    AnyView(Color.clear)
                }
            )
        case .split:
            return nodeView(workspace.root, focusedLeafId: workspace.focusedLeafId)
        }
    }

    /// Uses AnyView to break the recursive opaque return type inference.
    private func nodeView(_ node: PaneSplitNode, focusedLeafId: Int) -> AnyView {
        switch node {
        case .leaf(let id):
            return AnyView(
                PaneCellView(
                    workspaceId: workspaceId,
                    leafId: id,
                    isFocused: id == focusedLeafId
                )
            )

        case .split(let axis, let ratio, let first, let second):
            let splitId = first.firstLeafId
            return AnyView(
                DraggableSplit(
                    axis: axis, ratio: ratio,
                    onRatioChanged: { newRatio in
                        registry.setRatio(
                            newRatio, workspaceId: workspaceId, forSplitIdentifiedByFirstLeaf: splitId)
                    }
                ) {
                    nodeView(first, focusedLeafId: focusedLeafId)
                } second: {
                    nodeView(second, focusedLeafId: focusedLeafId)
                }
            )
        }
    }
}
