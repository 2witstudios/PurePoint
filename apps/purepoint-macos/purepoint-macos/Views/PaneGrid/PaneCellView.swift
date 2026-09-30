import SwiftUI

/// A single pane cell in the grid — shows a terminal or empty placeholder.
/// Hover detection lives on the outer ZStack (backed by the opaque terminal),
/// so the overlay never intercepts clicks meant for the terminal.
struct PaneCellView: View {
    let workspaceId: String
    let leafId: Int
    let agentId: String?
    let isFocused: Bool
    @State private var isHovered = false
    @Environment(AppState.self) private var appState
    @Environment(WorkspaceRegistry.self) private var registry
    @Environment(TerminalViewCache.self) private var viewCache

    private var workspace: Workspace? { registry.workspace(id: workspaceId) }

    var body: some View {
        ZStack(alignment: .topTrailing) {
            if let agentId, let agent = appState.agent(byId: agentId) {
                TerminalContainerView(
                    agent: agent,
                    isFocused: isFocused,
                    onFocus: { registry.setFocus(workspaceId: workspaceId, leafId: leafId) }
                )
            } else {
                PanePlaceholderView(workspaceId: workspaceId, leafId: leafId)
                    .onTapGesture {
                        registry.setFocus(workspaceId: workspaceId, leafId: leafId)
                    }
            }

            // Focus indicator bar — only meaningful once the workspace has more than one pane.
            if isFocused, (workspace?.paneCount ?? 1) > 1 {
                VStack {
                    Rectangle()
                        .fill(Color.accentColor)
                        .frame(height: 2)
                    Spacer()
                }
                .allowsHitTesting(false)
            }

            // Hover buttons (split/close)
            if isHovered, let workspace {
                HStack(spacing: 4) {
                    if workspace.root.canSplit(axis: .vertical) {
                        OverlayButton(icon: "rectangle.split.2x1", tooltip: "Split Right") {
                            split(axis: .vertical)
                        }
                    }
                    if workspace.root.canSplit(axis: .horizontal) {
                        OverlayButton(icon: "rectangle.split.1x2", tooltip: "Split Below") {
                            split(axis: .horizontal)
                        }
                    }
                    if workspace.paneCount > 1 {
                        OverlayButton(icon: "xmark", tooltip: "Close Pane") {
                            closePane()
                        }
                    }
                }
                .padding(6)
                .background(.ultraThinMaterial, in: RoundedRectangle(cornerRadius: 6))
                .padding(8)
                .transition(.opacity.animation(.easeInOut(duration: 0.2)))
            }
        }
        .onHover { hovering in
            withAnimation(.easeInOut(duration: 0.2).delay(hovering ? 0 : 0.3)) {
                isHovered = hovering
            }
        }
    }

    private func split(axis: PaneSplitNode.Axis) {
        registry.setFocus(workspaceId: workspaceId, leafId: leafId)
        registry.split(workspaceId: workspaceId, leafId: leafId, axis: axis)
        registry.pendingPaletteLeafId = registry.workspace(id: workspaceId)?.focusedLeafId
    }

    private func closePane() {
        if let agentId {
            viewCache.remove(agentId: agentId)
        }
        registry.closePane(workspaceId: workspaceId, leafId: leafId)
    }
}

/// Placeholder shown in empty panes — opens command palette to spawn a new agent.
private struct PanePlaceholderView: View {
    let workspaceId: String
    let leafId: Int
    @Environment(AppState.self) private var appState
    @Environment(WorkspaceRegistry.self) private var registry

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
            if registry.pendingPaletteLeafId == leafId {
                registry.pendingPaletteLeafId = nil
                // Defer so the view is laid out before the panel appears
                DispatchQueue.main.async {
                    openCommandPalette()
                }
            }
        }
    }

    private func openCommandPalette() {
        let state = appState
        let reg = registry
        let wsId = workspaceId
        let lid = leafId
        let hub = state.agentsHubState
        let items = CommandPaletteItem.buildItems(
            builtInVariants: AgentVariant.allVariants,
            agents: hub.agents,
            swarms: []
        )
        Task { await hub.loadAll(projectRoots: state.projects.map(\.projectRoot)) }

        CommandPalettePanel.show(relativeTo: NSApp.keyWindow, items: items) { result in
            guard let projectRoot = reg.projectRoot(forWorkspace: wsId),
                let project = state.projectState(forRoot: projectRoot)
            else { return }

            switch result {
            case .spawnBuiltIn(let variant, let prompt, _):
                project.spawnAgentForPane(
                    agent: variant.id, prompt: prompt ?? "", workspaceId: wsId, leafId: lid)
            case .spawnAgentDef(let def, let prompt):
                project.spawnAgentForPane(
                    agent: def.agentType, prompt: prompt ?? def.inlinePrompt ?? "",
                    workspaceId: wsId, leafId: lid)
            case .runSwarm, .createWorktree:
                break
            }
        }
    }
}
