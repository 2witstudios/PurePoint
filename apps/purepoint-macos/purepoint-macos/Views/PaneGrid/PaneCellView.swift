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
                TerminalContainerView(agent: agent, isFocused: isFocused)
            } else {
                PanePlaceholderView(leafId: leafId)
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
}

/// Placeholder shown in empty panes — opens command palette to spawn a new agent.
private struct PanePlaceholderView: View {
    let leafId: Int
    @Environment(AppState.self) private var appState
    @Environment(GridState.self) private var gridState

    var body: some View {
        VStack(spacing: 12) {
            Image(systemName: "rectangle.dashed")
                .font(.system(size: 32))
                .foregroundStyle(.quaternary)
            Text("Empty Pane")
                .font(.title3)
                .foregroundStyle(.tertiary)
            Text("Spawn a new agent")
                .font(.caption)
                .foregroundStyle(.quaternary)
            Button("New Agent\u{2026}") {
                openCommandPalette()
            }
            .buttonStyle(.bordered)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .onAppear {
            if gridState.pendingPaletteLeafId == leafId {
                gridState.pendingPaletteLeafId = nil
                // Defer so the view is laid out before the panel appears
                DispatchQueue.main.async {
                    openCommandPalette()
                }
            }
        }
    }

    private func openCommandPalette() {
        let state = appState
        let gs = gridState
        let lid = leafId
        let hub = state.agentsHubState
        let items = CommandPaletteItem.buildItems(
            builtInVariants: AgentVariant.allVariants,
            agents: hub.agents,
            swarms: []
        )
        if let root = gs.projectRoot {
            Task { await hub.loadAll(projectRoot: root) }
        }

        CommandPalettePanel.show(relativeTo: NSApp.keyWindow, items: items) { result in
            let project = state.projectState(forRoot: gs.projectRoot ?? "")
            switch result {
            case .spawnBuiltIn(let variant, let prompt, _):
                project?.spawnAgentForPane(agent: variant.id, prompt: prompt ?? "", leafId: lid, gridState: gs)
            case .spawnAgentDef(let def, let prompt):
                project?.spawnAgentForPane(agent: def.agentType, prompt: prompt ?? def.inlinePrompt ?? "", leafId: lid, gridState: gs)
            case .runSwarm:
                break
            }
        }
    }
}

/// Hover overlay showing split/close buttons in the top-right corner.
private struct HoverOverlay: View {
    let leafId: Int
    @State private var isHovered = false
    @Environment(GridState.self) private var gridState

    var body: some View {
        ZStack(alignment: .topTrailing) {
            Color.clear

            if isHovered {
                HStack(spacing: 4) {
                    if gridState.canSplit(axis: .vertical) {
                        OverlayButton(icon: "rectangle.split.2x1", tooltip: "Split Right") {
                            gridState.focusedLeafId = leafId
                            gridState.splitFocused(axis: .vertical)
                            gridState.pendingPaletteLeafId = gridState.focusedLeafId
                        }
                    }
                    if gridState.canSplit(axis: .horizontal) {
                        OverlayButton(icon: "rectangle.split.1x2", tooltip: "Split Below") {
                            gridState.focusedLeafId = leafId
                            gridState.splitFocused(axis: .horizontal)
                            gridState.pendingPaletteLeafId = gridState.focusedLeafId
                        }
                    }
                    if gridState.leafCount > 1 {
                        OverlayButton(icon: "xmark", tooltip: "Close Pane") {
                            gridState.focusedLeafId = leafId
                            gridState.closeFocused()
                        }
                    }
                }
                .padding(6)
                .background(.ultraThinMaterial, in: RoundedRectangle(cornerRadius: 6))
                .padding(8)
                .transition(.opacity.animation(.easeInOut(duration: 0.2)))
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .contentShape(Rectangle())
        .onHover { hovering in
            withAnimation(.easeInOut(duration: 0.2).delay(hovering ? 0 : 0.3)) {
                isHovered = hovering
            }
        }
    }
}
