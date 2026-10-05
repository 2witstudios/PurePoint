import SwiftUI

/// A single pane cell in the grid: its tab strip above the active tab's content —
/// a terminal, the file navigator, or the empty-tab placeholder.
///
/// Only the active tab is rendered. A background agent's terminal view stays alive in
/// `TerminalViewCache` (keyed by agent ID), so switching back is instant and keeps scrollback.
struct PaneCellView: View {
    let workspaceId: String
    let leafId: Int
    let isFocused: Bool
    @Environment(AppState.self) private var appState
    @Environment(WorkspaceRegistry.self) private var registry

    private var workspace: Workspace? { registry.workspace(id: workspaceId) }

    private var activeTab: Surface? { workspace?.panes[leafId]?.activeTab }

    private var fileRootPath: String? { appState.fileRoot(forWorkspace: workspaceId, registry: registry) }

    var body: some View {
        VStack(spacing: 0) {
            PaneTabBar(workspaceId: workspaceId, leafId: leafId, isFocused: isFocused)
            content
                .frame(maxWidth: .infinity, maxHeight: .infinity)
        }
    }

    @ViewBuilder
    private var content: some View {
        switch activeTab?.content {
        case .agent(let agentId):
            if let agent = appState.agent(byId: agentId) {
                TerminalContainerView(agent: agent, isFocused: isFocused, onFocus: focus)
            } else {
                // Bound to an agent the manifest has not delivered yet.
                Color(nsColor: TerminalTheme.background)
                    .onTapGesture(perform: focus)
            }
        case .file(let path):
            if let rootPath = fileRootPath, let surface = activeTab {
                FilePaneView(
                    workspaceId: workspaceId, leafId: leafId, rootPath: rootPath,
                    initialPath: path, onFocus: focus
                )
                // Each file tab keeps its own navigator and editor state.
                .id(surface.id)
            }
        case .empty, nil:
            if let surface = activeTab {
                EmptyTabView(workspaceId: workspaceId, surfaceId: surface.id)
                    .id(surface.id)
                    .onTapGesture(perform: focus)
            }
        }
    }

    private func focus() {
        registry.setFocus(workspaceId: workspaceId, leafId: leafId)
    }
}

/// An empty tab — opens the command palette to spawn an agent or open files into it.
private struct EmptyTabView: View {
    let workspaceId: String
    let surfaceId: Int
    @Environment(AppState.self) private var appState
    @Environment(WorkspaceRegistry.self) private var registry

    var body: some View {
        VStack(spacing: 12) {
            Image(systemName: "rectangle.dashed")
                .font(.system(size: 32))
                .foregroundStyle(.quaternary)
            Text("Empty Tab")
                .font(.title3)
                .foregroundStyle(.tertiary)
            Text("Spawn a new agent, or open files")
                .font(.caption)
                .foregroundStyle(.quaternary)
            Button("New Agent\u{2026}") {
                openCommandPalette()
            }
            .buttonStyle(.bordered)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .onAppear(perform: openPaletteIfPending)
        .onChange(of: registry.pendingPaletteSurfaceId) { _, _ in openPaletteIfPending() }
    }

    /// A split or ⌘T marks the tab it created; that tab opens the palette once it is on screen.
    private func openPaletteIfPending() {
        guard registry.pendingPaletteSurfaceId == surfaceId else { return }
        registry.pendingPaletteSurfaceId = nil
        // Defer so the view is laid out before the panel appears
        DispatchQueue.main.async {
            openCommandPalette()
        }
    }

    private func openCommandPalette() {
        let state = appState
        let reg = registry
        let wsId = workspaceId
        let sid = surfaceId
        let hub = state.agentsHubState
        let rootPath = state.fileRoot(forWorkspace: wsId, registry: reg)
        Task { await hub.loadAll(projectRoots: state.projects.map(\.projectRoot)) }
        Task {
            let files = await Task.detached { rootPath.map { FileIndex.list(root: $0) } ?? [] }.value
            let items = CommandPaletteItem.buildItems(
                builtInVariants: AgentVariant.allVariants,
                agents: hub.agents,
                swarms: [],
                includeFiles: true,
                files: files
            )
            showPalette(items: items, state: state, reg: reg, wsId: wsId, sid: sid)
        }
    }

    private func showPalette(
        items: [CommandPaletteItem], state: AppState, reg: WorkspaceRegistry, wsId: String, sid: Int
    ) {
        CommandPalettePanel.show(relativeTo: NSApp.keyWindow, items: items) { result in
            guard let projectRoot = reg.projectRoot(forWorkspace: wsId),
                let project = state.projectState(forRoot: projectRoot)
            else { return }

            switch result {
            case .spawnBuiltIn(let variant, let prompt, _):
                project.spawnAgentForSurface(
                    agent: variant.id, prompt: prompt ?? "", workspaceId: wsId, surfaceId: sid)
            case .spawnAgentDef(let def, let prompt):
                project.spawnAgentForSurface(
                    agent: def.agentType, prompt: prompt ?? def.inlinePrompt ?? "",
                    workspaceId: wsId, surfaceId: sid)
            case .runSwarm, .createWorktree:
                break
            case .openFilePane(let path):
                reg.openFile(workspaceId: wsId, surfaceId: sid, path: path)
            }
        }
    }
}
