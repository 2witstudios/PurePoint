import AppKit
import SwiftUI

/// The strip at the top of every pane: its tabs, then the pane's actions.
///
/// Always shown, even for a single tab, so a background agent's state is visible without
/// switching to it and the pane actions never hide behind a hover.
struct PaneTabBar: View {
    let workspaceId: String
    let leafId: Int
    let isFocused: Bool
    @Environment(AppState.self) private var appState
    @Environment(WorkspaceRegistry.self) private var registry
    @State private var isDropTargeted = false

    static let height: CGFloat = 30

    private var workspace: Workspace? { registry.workspace(id: workspaceId) }
    private var pane: Pane? { workspace?.panes[leafId] }

    var body: some View {
        HStack(spacing: 0) {
            ScrollView(.horizontal, showsIndicators: false) {
                HStack(spacing: 0) {
                    ForEach(pane?.tabs ?? []) { surface in
                        PaneTab(
                            workspaceId: workspaceId,
                            surface: surface,
                            isActive: surface.id == pane?.activeTabId,
                            isFocusedPane: isFocused
                        )
                    }
                }
            }
            // Dropping past the last tab appends to this pane.
            .dropDestination(for: String.self) { items, _ in
                guard let surfaceId = items.lazy.compactMap(TabDragPayload.surfaceId(from:)).first else { return false }
                registry.moveTab(workspaceId: workspaceId, surfaceId: surfaceId, toLeaf: leafId)
                return true
            } isTargeted: { isDropTargeted = $0 }

            Divider()
            actions
        }
        .frame(height: Self.height)
        .background(isDropTargeted ? Color.accentColor.opacity(0.12) : Color(nsColor: Theme.tabBarBackground))
        .overlay(alignment: .bottom) { Divider() }
        .environment(\.colorScheme, .dark)
    }

    private var actions: some View {
        HStack(spacing: 2) {
            TabBarButton(icon: "plus", help: "New Tab") { newTab() }
            if let workspace, workspace.root.canSplit(axis: .vertical) {
                TabBarButton(icon: "rectangle.split.2x1", help: "Split Right") { split(.vertical) }
            }
            if let workspace, workspace.root.canSplit(axis: .horizontal) {
                TabBarButton(icon: "rectangle.split.1x2", help: "Split Below") { split(.horizontal) }
            }
            if (workspace?.paneCount ?? 1) > 1 {
                TabBarButton(icon: "xmark.rectangle", help: "Close Pane") { closePane() }
            }
        }
        .padding(.horizontal, 4)
    }

    private func newTab() {
        registry.pendingPaletteSurfaceId = registry.newTab(workspaceId: workspaceId, leafId: leafId)
    }

    private func split(_ axis: PaneSplitNode.Axis) {
        registry.setFocus(workspaceId: workspaceId, leafId: leafId)
        registry.split(workspaceId: workspaceId, leafId: leafId, axis: axis)
        registry.pendingPaletteSurfaceId = registry.workspace(id: workspaceId)?.focusedSurface?.id
    }

    private func closePane() {
        guard let pane, PaneCloseConfirmation.confirm(pane: pane, appState: appState) else { return }
        registry.closePane(workspaceId: workspaceId, leafId: leafId)
    }
}

/// One tab: kind/status icon, name, and a trailing slot that shows the close button on the
/// active or hovered tab and the unseen-output dot otherwise.
private struct PaneTab: View {
    let workspaceId: String
    let surface: Surface
    let isActive: Bool
    let isFocusedPane: Bool
    @Environment(AppState.self) private var appState
    @Environment(WorkspaceRegistry.self) private var registry
    @Environment(TerminalViewCache.self) private var viewCache
    @State private var isHovered = false
    @State private var isDropTargeted = false

    private var agent: AgentModel? { surface.content.agentId.flatMap { appState.agent(byId: $0) } }

    private var hasUnseenOutput: Bool {
        guard !isActive, let agentId = surface.content.agentId else { return false }
        return viewCache.unseenOutput.contains(agentId)
    }

    var body: some View {
        HStack(spacing: 6) {
            icon
                .font(.system(size: 11, weight: .medium))
                .frame(width: 14)
            Text(title)
                .font(.system(size: 12))
                .lineLimit(1)
                .truncationMode(.tail)
            trailingSlot
                .frame(width: 16, height: 16)
        }
        .padding(.leading, 10)
        .padding(.trailing, 6)
        .frame(minWidth: 84, maxWidth: 200, maxHeight: .infinity)
        .foregroundStyle(textStyle)
        .background(isActive ? Color(nsColor: TerminalTheme.background) : Color.clear)
        .overlay(alignment: .top) {
            if isActive && isFocusedPane {
                Rectangle().fill(Color.accentColor).frame(height: 2)
            }
        }
        .overlay(alignment: .leading) {
            if isDropTargeted {
                Rectangle().fill(Color.accentColor).frame(width: 2)
            }
        }
        .overlay(alignment: .trailing) { Divider() }
        .contentShape(Rectangle())
        .onHover { isHovered = $0 }
        .onTapGesture { registry.selectTab(workspaceId: workspaceId, surfaceId: surface.id) }
        .onMiddleClick { close() }
        .help(helpText)
        .draggable(TabDragPayload.string(for: surface.id)) {
            Text(title).font(.system(size: 12)).padding(6)
        }
        // Dropping on a tab inserts before it.
        .dropDestination(for: String.self) { items, _ in
            guard let moved = items.lazy.compactMap(TabDragPayload.surfaceId(from:)).first,
                moved != surface.id,
                let workspace = registry.workspace(id: workspaceId),
                let leafId = workspace.leafId(ofSurface: surface.id),
                var index = workspace.panes[leafId]?.index(of: surface.id)
            else { return false }
            // Removing a tab that sat earlier in this same pane shifts everything after it left.
            if workspace.leafId(ofSurface: moved) == leafId,
                let from = workspace.panes[leafId]?.index(of: moved), from < index
            {
                index -= 1
            }
            registry.moveTab(workspaceId: workspaceId, surfaceId: moved, toLeaf: leafId, index: index)
            return true
        } isTargeted: { isDropTargeted = $0 }
        .accessibilityElement(children: .combine)
        .accessibilityAddTraits(isActive ? [.isButton, .isSelected] : .isButton)
        .accessibilityLabel(helpText)
        .accessibilityAction(named: "Close Tab") { close() }
    }

    // MARK: - Pieces

    @ViewBuilder
    private var icon: some View {
        switch surface.content {
        case .agent:
            if let agent, agent.agentType == "terminal" {
                Image(systemName: "terminal")
                    .foregroundStyle(agent.status.isAlive ? Color.secondary : Color(nsColor: .systemRed))
            } else if let agent, !agent.status.isAlive {
                Image(systemName: "xmark.circle").foregroundStyle(Color(nsColor: .systemRed))
            } else {
                Image(systemName: "sparkle").foregroundStyle(Color(nsColor: .systemGreen))
            }
        case .file:
            Image(systemName: "doc.text").foregroundStyle(.secondary)
        case .empty:
            Image(systemName: "magnifyingglass").foregroundStyle(.secondary)
        }
    }

    @ViewBuilder
    private var trailingSlot: some View {
        if isActive || isHovered {
            Button(action: close) {
                Image(systemName: "xmark")
                    .font(.system(size: 9, weight: .semibold))
                    .frame(width: 16, height: 16)
                    .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .foregroundStyle(.secondary)
            .accessibilityLabel("Close Tab")
        } else if hasUnseenOutput {
            Circle()
                .fill(Color.accentColor)
                .frame(width: 6, height: 6)
                .accessibilityHidden(true)
        }
    }

    private var title: String {
        switch surface.content {
        case .agent(let agentId):
            return agent?.displayName ?? agentId
        case .file(let path):
            return path.map { ($0 as NSString).lastPathComponent } ?? "Files"
        case .empty:
            return "New Tab"
        }
    }

    private var textStyle: Color {
        if isActive { return isFocusedPane ? .primary : .primary.opacity(0.8) }
        return hasUnseenOutput ? .primary.opacity(0.9) : .secondary
    }

    private var helpText: String {
        var parts = [title]
        if let agent {
            parts.append(agent.status.isAlive ? "running" : "exited")
        }
        if hasUnseenOutput { parts.append("new output") }
        return parts.joined(separator: " — ")
    }

    private func close() {
        registry.closeTab(workspaceId: workspaceId, surfaceId: surface.id)
    }
}

private struct TabBarButton: View {
    let icon: String
    let help: String
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            Image(systemName: icon)
                .font(.system(size: 11))
                .frame(width: 24, height: 22)
                .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .foregroundStyle(.secondary)
        .help(help)
        .accessibilityLabel(help)
    }
}

/// Drag payload for a tab. Tabs only move within a workspace, and surface IDs are unique
/// there, so the ID alone identifies the tab; the prefix keeps stray text drops out.
enum TabDragPayload {
    private static let prefix = "purepoint-tab:"

    static func string(for surfaceId: Int) -> String { prefix + String(surfaceId) }

    static func surfaceId(from string: String) -> Int? {
        guard string.hasPrefix(prefix) else { return nil }
        return Int(string.dropFirst(prefix.count))
    }
}

/// Closing a pane stops every agent in its tabs. One is what closing a tab does anyway;
/// more than one is easy to do by accident, so it asks first.
@MainActor
enum PaneCloseConfirmation {
    static func confirm(pane: Pane, appState: AppState) -> Bool {
        let running = pane.tabs.compactMap(\.content.agentId).filter {
            appState.agent(byId: $0)?.status.isAlive ?? false
        }
        guard running.count > 1 else { return true }

        let alert = NSAlert()
        alert.messageText = "Close pane and stop \(running.count) agents?"
        alert.informativeText = "Every tab in this pane closes, and the agents running in them are stopped."
        alert.alertStyle = .warning
        alert.addButton(withTitle: "Close Pane")
        alert.addButton(withTitle: "Cancel")
        return alert.runModal() == .alertFirstButtonReturn
    }
}

private struct MiddleClickModifier: ViewModifier {
    let action: () -> Void

    func body(content: Content) -> some View {
        content.overlay(MiddleClickCatcher(action: action))
    }
}

/// Catches only the middle mouse button; every other event passes through to SwiftUI.
private struct MiddleClickCatcher: NSViewRepresentable {
    let action: () -> Void

    func makeNSView(context: Context) -> CatcherView {
        let view = CatcherView()
        view.action = action
        return view
    }

    func updateNSView(_ nsView: CatcherView, context: Context) {
        nsView.action = action
    }

    final class CatcherView: NSView {
        var action: (() -> Void)?

        override func hitTest(_ point: NSPoint) -> NSView? {
            NSApp.currentEvent?.type == .otherMouseDown ? super.hitTest(point) : nil
        }

        override func otherMouseDown(with event: NSEvent) {
            if event.buttonNumber == 2 { action?() }
        }
    }
}

extension View {
    fileprivate func onMiddleClick(perform action: @escaping () -> Void) -> some View {
        modifier(MiddleClickModifier(action: action))
    }
}
