import SwiftUI

/// A single pane cell in the grid — shows a terminal or empty placeholder.
struct PaneCellView: View {
    let leafId: Int
    let agentId: String?
    let isFocused: Bool
    @Environment(AppState.self) private var appState
    @Environment(GridState.self) private var gridState

    var body: some View {
        ZStack(alignment: .top) {
            if let agentId, let agent = appState.agent(byId: agentId) {
                TerminalContainerView(agent: agent)
            } else {
                placeholderView
            }

            // Focus indicator bar
            if isFocused {
                Rectangle()
                    .fill(Color.accentColor)
                    .frame(height: 2)
            }

            // Hover overlay with split/close buttons
            HoverOverlay(leafId: leafId)
        }
        .contentShape(Rectangle())
        .onTapGesture {
            gridState.focusedLeafId = leafId
        }
    }

    private var placeholderView: some View {
        VStack(spacing: 8) {
            Text("Empty Pane")
                .font(.title3)
                .foregroundStyle(.tertiary)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}

/// Hover overlay showing split/close buttons in the top-right corner.
private struct HoverOverlay: View {
    let leafId: Int
    @State private var isHovered = false
    @Environment(GridState.self) private var gridState

    var body: some View {
        HStack {
            Spacer()
            if isHovered {
                HStack(spacing: 4) {
                    if gridState.canSplit(axis: .vertical) {
                        overlayButton(icon: "rectangle.split.1x2", tooltip: "Split Right") {
                            gridState.splitFocused(axis: .vertical)
                        }
                    }
                    if gridState.canSplit(axis: .horizontal) {
                        overlayButton(icon: "rectangle.split.2x1", tooltip: "Split Below") {
                            gridState.splitFocused(axis: .horizontal)
                        }
                    }
                    overlayButton(icon: "xmark", tooltip: "Close Pane") {
                        gridState.closeFocused()
                    }
                }
                .padding(6)
                .background(.ultraThinMaterial, in: RoundedRectangle(cornerRadius: 6))
                .padding(8)
            }
        }
        .frame(maxWidth: .infinity, alignment: .trailing)
        .onHover { isHovered = $0 }
    }

    private func overlayButton(icon: String, tooltip: String, action: @escaping () -> Void) -> some View {
        Button(action: action) {
            Image(systemName: icon)
                .font(.system(size: 11))
                .frame(width: 20, height: 20)
        }
        .buttonStyle(.plain)
        .help(tooltip)
    }
}
