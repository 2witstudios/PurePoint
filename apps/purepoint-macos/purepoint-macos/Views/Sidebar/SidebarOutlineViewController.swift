import AppKit

/// NSViewController hosting an NSOutlineView for compact sidebar rows.
/// Combines data source, delegate, and cell factories.
@MainActor
class SidebarOutlineViewController: NSViewController, NSOutlineViewDataSource, NSOutlineViewDelegate {

    let scrollView = NSScrollView()
    let outlineView = NSOutlineView()

    var projectNodes: [SidebarNode] = []
    private var lastRenderState: SidebarRenderState?

    /// Callback when user clicks a row — maps to SidebarSelection.
    var onSelectionChanged: ((SidebarSelection?) -> Void)?

    /// Callback for showing the command palette for a project+selection context.
    var onShowCommandPalette: ((ProjectState, SidebarSelection?, Bool) -> Void)?

    /// Callback for creating a terminal in a worktree.
    var onAddTerminal: ((ProjectState, WorktreeModel) -> Void)?

    /// Callback for killing every agent in a workspace: (project, workspaceId).
    var onKillWorkspace: ((ProjectState, String) -> Void)?

    /// Callback for killing all agents in a worktree.
    var onKillWorktreeAgents: ((ProjectState, String) -> Void)?

    /// Callback for renaming a workspace's primary agent: (project, agentId, newName).
    var onRenameAgent: ((ProjectState, String, String) -> Void)?

    /// Callback for deleting a worktree (full cleanup).
    var onDeleteWorktree: ((ProjectState, String) -> Void)?

    /// Callback for killing all agents in a project.
    var onKillAllProjectAgents: ((ProjectState) -> Void)?

    /// Callback for removing a project from the sidebar.
    var onRemoveProject: ((ProjectState) -> Void)?

    /// Canonical workspaces per project root. The sidebar renders these and nothing else —
    /// there is no agent list to filter, so a pane cannot leak out as its own row.
    var workspacesByProject: [String: [Workspace]] = [:]

    /// The workspace whose grid is on screen, for the active-row marker.
    var activeWorkspaceId: String?

    /// Prevents feedback loops during programmatic selection changes.
    private var suppressSelectionCallback = false

    /// Node for context-menu tracking.
    private var contextClickedNode: SidebarNode?

    /// Inline rename state.
    private var editingTextField: NSTextField?
    private var isStartingRename = false
    private var editingOriginalName: String?
    private var editingWorkspaceId: String?

    private struct SidebarRenderState: Equatable {
        let activeWorkspaceId: String?
        let projects: [ProjectRenderState]
    }

    private struct ProjectRenderState: Equatable {
        let channelUnread: Int
        let projectRoot: String
        let rootWorkspaces: [WorkspaceRenderState]
        let worktrees: [WorktreeRenderState]
    }

    private struct WorktreeRenderState: Equatable {
        let id: String
        let branch: String
        let workspaces: [WorkspaceRenderState]
    }

    private struct WorkspaceRenderState: Equatable {
        let id: String
        let title: String
        let paneCount: Int
        let tabCount: Int
        /// `nil` for a workspace holding only file tabs — it has no agent to report on.
        let status: AgentStatus?
    }

    // MARK: - Lifecycle

    override func loadView() {
        view = NSView()
    }

    override func viewDidLoad() {
        super.viewDidLoad()

        let column = NSTableColumn(identifier: NSUserInterfaceItemIdentifier("main"))
        column.title = ""
        outlineView.addTableColumn(column)
        outlineView.outlineTableColumn = column
        outlineView.headerView = nil
        outlineView.dataSource = self
        outlineView.delegate = self
        outlineView.rowSizeStyle = .custom
        outlineView.style = .sourceList
        outlineView.indentationPerLevel = 12
        outlineView.backgroundColor = .clear

        let contextMenu = NSMenu()
        contextMenu.delegate = self
        outlineView.menu = contextMenu

        scrollView.documentView = outlineView
        scrollView.hasVerticalScroller = true
        scrollView.drawsBackground = false
        scrollView.translatesAutoresizingMaskIntoConstraints = false
        view.addSubview(scrollView)

        NSLayoutConstraint.activate([
            scrollView.topAnchor.constraint(equalTo: view.topAnchor),
            scrollView.leadingAnchor.constraint(equalTo: view.leadingAnchor),
            scrollView.trailingAnchor.constraint(equalTo: view.trailingAnchor),
            scrollView.bottomAnchor.constraint(equalTo: view.bottomAnchor),
        ])
    }

    // MARK: - Data Rebuild

    /// Rebuild the node tree from AppState projects.
    func rebuildNodes(projects: [ProjectState]) {
        // Reloading mid-rename discards the cell holding the field editor, leaving it dead.
        // `lastRenderState` is untouched, so the next update after the edit rebuilds.
        guard editingTextField == nil else { return }
        let nextRenderState = makeRenderState(projects: projects)
        guard nextRenderState != lastRenderState else { return }

        let oldSelectedId = selectedNodeId()
        let scrollOrigin = scrollView.contentView.bounds.origin

        projectNodes = projects.map { buildProjectNode(from: $0) }
        lastRenderState = nextRenderState
        outlineView.reloadData()
        restoreExpansionState()

        if let oldSelectedId {
            selectNode(withId: oldSelectedId)
        }

        scrollView.contentView.scroll(to: scrollOrigin)
        scrollView.reflectScrolledClipView(scrollView.contentView)
    }

    private func buildProjectNode(from project: ProjectState) -> SidebarNode {
        let workspaces = workspacesByProject[project.projectRoot] ?? []

        var projectChildren: [SidebarNode] = workspaces
            .filter { $0.container == .projectRoot }
            .map { SidebarNode(kind: .workspace($0)) }

        for worktree in project.worktrees {
            let children = workspaces
                .filter { $0.container == .worktree(worktree.id) }
                .map { SidebarNode(kind: .workspace($0)) }
            projectChildren.append(SidebarNode(kind: .worktree(worktree), children: children))
        }

        return SidebarNode(kind: .project(project), children: projectChildren)
    }

    /// How a workspace presents itself as one row: named after its first pane's agent,
    /// with the pane count when it holds more than one.
    private func renderState(for workspace: Workspace, in project: ProjectState) -> WorkspaceRenderState {
        let agents = workspace.agentIds.compactMap { project.agent(byId: $0) }
        let firstFile = workspace.surfaces.lazy.compactMap { entry -> String? in
            guard case .file(let path) = entry.surface.content else { return nil }
            return path.map { ($0 as NSString).lastPathComponent } ?? "Files"
        }.first
        let title = agents.first?.displayName ?? workspace.primaryAgentId ?? firstFile ?? workspace.id
        let status: AgentStatus? =
            agents.isEmpty
            ? nil : agents.contains { !$0.status.isAlive } ? .broken : (agents.first?.status ?? .running)
        return WorkspaceRenderState(
            id: workspace.id, title: title, paneCount: workspace.paneCount, tabCount: workspace.tabCount,
            status: status)
    }

    private func makeRenderState(projects: [ProjectState]) -> SidebarRenderState {
        let projectStates = projects.map { project in
            let workspaces = workspacesByProject[project.projectRoot] ?? []

            let rootWorkspaces = workspaces
                .filter { $0.container == .projectRoot }
                .map { renderState(for: $0, in: project) }

            let worktrees = project.worktrees.map { worktree in
                WorktreeRenderState(
                    id: worktree.id,
                    branch: worktree.branch,
                    workspaces: workspaces
                        .filter { $0.container == .worktree(worktree.id) }
                        .map { renderState(for: $0, in: project) }
                )
            }

            return ProjectRenderState(
                channelUnread: project.channel.unreadCount,
                projectRoot: project.projectRoot,
                rootWorkspaces: rootWorkspaces,
                worktrees: worktrees
            )
        }

        return SidebarRenderState(activeWorkspaceId: activeWorkspaceId, projects: projectStates)
    }

    private func restoreExpansionState() {
        for node in projectNodes {
            outlineView.expandItem(node)
            for child in node.children {
                if case .worktree = child.kind {
                    outlineView.expandItem(child)
                }
            }
        }
    }

    private func selectedNodeId() -> String? {
        let row = outlineView.selectedRow
        guard row >= 0, let node = outlineView.item(atRow: row) as? SidebarNode else { return nil }
        return node.id
    }

    // MARK: - Programmatic Selection

    func selectNode(for selection: SidebarSelection?) {
        guard let selection else {
            deselectAll()
            return
        }

        let targetId: String
        switch selection {
        case .workspace(let id): targetId = id
        case .worktree(let id): targetId = id
        case .channel(let root): targetId = root
        case .project(let root): targetId = root
        case .nav:
            deselectAll()
            return
        }

        selectNode(withId: targetId)
    }

    private func deselectAll() {
        suppressSelectionCallback = true
        outlineView.deselectAll(nil)
        suppressSelectionCallback = false
    }

    private func selectNode(withId targetId: String) {
        if selectedNodeId() == targetId {
            return
        }

        for row in 0..<outlineView.numberOfRows {
            if let node = outlineView.item(atRow: row) as? SidebarNode, node.id == targetId {
                suppressSelectionCallback = true
                outlineView.selectRowIndexes(IndexSet(integer: row), byExtendingSelection: false)
                suppressSelectionCallback = false
                return
            }
        }
    }

    // MARK: - NSOutlineViewDataSource

    func outlineView(_ outlineView: NSOutlineView, numberOfChildrenOfItem item: Any?) -> Int {
        if item == nil { return projectNodes.count }
        if let node = item as? SidebarNode { return node.children.count }
        return 0
    }

    func outlineView(_ outlineView: NSOutlineView, child index: Int, ofItem item: Any?) -> Any {
        if item == nil { return projectNodes[index] }
        if let node = item as? SidebarNode { return node.children[index] }
        fatalError("Unexpected item type")
    }

    func outlineView(_ outlineView: NSOutlineView, isItemExpandable item: Any) -> Bool {
        guard let node = item as? SidebarNode else { return false }
        switch node.kind {
        case .project, .worktree: return true
        case .workspace: return false
        }
    }

    // MARK: - NSOutlineViewDelegate

    func outlineView(_ outlineView: NSOutlineView, viewFor tableColumn: NSTableColumn?, item: Any) -> NSView? {
        guard let node = item as? SidebarNode else { return nil }
        switch node.kind {
        case .project(let project): return makeProjectCell(project)
        case .worktree(let worktree): return makeWorktreeCell(worktree, node: node)
        case .workspace(let workspace): return makeWorkspaceCell(workspace, node: node)
        }
    }

    func outlineView(_ outlineView: NSOutlineView, heightOfRowByItem item: Any) -> CGFloat {
        PurePointTheme.sidebarRowHeight
    }

    func outlineViewSelectionDidChange(_ notification: Notification) {
        guard !suppressSelectionCallback else { return }

        let row = outlineView.selectedRow
        guard row >= 0, let node = outlineView.item(atRow: row) as? SidebarNode else {
            onSelectionChanged?(nil)
            return
        }

        let selection: SidebarSelection
        switch node.kind {
        case .project(let p): selection = .project(p.projectRoot)
        case .worktree(let w): selection = .worktree(w.id)
        case .workspace(let w): selection = .workspace(w.id)
        }
        onSelectionChanged?(selection)
    }

    // MARK: - Cell Factories

    private func makeProjectCell(_ project: ProjectState) -> NSView {
        let (cell, stack) = makeCellWithStack(spacing: 6)

        let icon = makeSymbolIcon("folder.fill", description: "Project", pointSize: 11)
        let name = NSTextField(labelWithString: project.projectName)
        name.font = .systemFont(ofSize: 12, weight: .semibold)
        name.lineBreakMode = .byTruncatingTail

        let addBtn = makeInlineAddButton(action: #selector(projectAddClicked(_:)))
        addBtn.identifier = NSUserInterfaceItemIdentifier(project.projectRoot)

        stack.addArrangedSubview(icon)
        stack.addArrangedSubview(name)
        stack.addArrangedSubview(spacerView())
        if project.channel.unreadCount > 0 {
            let badge = makeBadge(count: project.channel.unreadCount)
            badge.identifier = NSUserInterfaceItemIdentifier("channelUnreadCount")
            badge.textColor = .controlAccentColor
            badge.setAccessibilityLabel("\(project.channel.unreadCount) unread channel messages")
            stack.addArrangedSubview(badge)
        }
        stack.addArrangedSubview(addBtn)
        return cell
    }

    private func makeWorktreeCell(_ worktree: WorktreeModel, node: SidebarNode) -> NSView {
        let (cell, stack) = makeCellWithStack(spacing: 6)

        let icon = makeSymbolIcon("arrow.triangle.branch", description: "Worktree", pointSize: 10)
        let name = NSTextField(labelWithString: worktree.branch)
        name.font = .systemFont(ofSize: 12)
        name.lineBreakMode = .byTruncatingTail

        let addBtn = makeInlineAddButton(action: #selector(worktreeAddClicked(_:)))
        addBtn.identifier = NSUserInterfaceItemIdentifier(worktree.id)

        stack.addArrangedSubview(icon)
        stack.addArrangedSubview(name)
        stack.addArrangedSubview(spacerView())
        stack.addArrangedSubview(addBtn)

        if worktree.agents.count > 0 {
            stack.addArrangedSubview(makeBadge(count: worktree.agents.count))
        }
        return cell
    }

    /// One row per workspace. A multi-pane workspace shows a pane count instead of
    /// expanding into child rows — its panes are only ever reachable inside its grid.
    private func makeWorkspaceCell(_ workspace: Workspace, node: SidebarNode) -> NSView {
        let (cell, stack) = makeCellWithStack(spacing: 5)

        guard let project = findProject(forWorkspaceId: workspace.id) else { return cell }
        let state = renderState(for: workspace, in: project)

        stack.addArrangedSubview(makeStatusDot(color: state.status?.nsColor ?? .tertiaryLabelColor))

        let label = NSTextField(labelWithString: state.title)
        label.font = .systemFont(ofSize: 11)
        label.lineBreakMode = .byTruncatingTail
        label.identifier = Self.workspaceNameLabelId
        stack.addArrangedSubview(label)

        if state.paneCount > 1 || state.tabCount > 1 {
            stack.addArrangedSubview(spacerView())
        }
        if state.paneCount > 1 {
            stack.addArrangedSubview(makeCountIcon("rectangle.split.2x2", description: "Panes", count: state.paneCount))
        }
        if state.tabCount > state.paneCount {
            stack.addArrangedSubview(makeCountIcon("square.stack", description: "Tabs", count: state.tabCount))
        }

        let paneNote = state.paneCount > 1 ? ", \(state.paneCount) panes" : ""
        let tabNote = state.tabCount > state.paneCount ? ", \(state.tabCount) tabs" : ""
        cell.setAccessibilityLabel("\(state.title), \(state.status?.rawValue ?? "files")\(paneNote)\(tabNote)")
        return cell
    }

    // MARK: - Cell Component Helpers

    private func makeCellWithStack(spacing: CGFloat) -> (NSTableCellView, NSStackView) {
        let cell = NSTableCellView()
        let stack = NSStackView()
        stack.orientation = .horizontal
        stack.spacing = spacing
        stack.translatesAutoresizingMaskIntoConstraints = false
        cell.addSubview(stack)
        NSLayoutConstraint.activate([
            stack.leadingAnchor.constraint(equalTo: cell.leadingAnchor, constant: 2),
            stack.trailingAnchor.constraint(equalTo: cell.trailingAnchor, constant: -4),
            stack.centerYAnchor.constraint(equalTo: cell.centerYAnchor),
        ])
        return (cell, stack)
    }

    private func makeSymbolIcon(_ name: String, description: String, pointSize: CGFloat) -> NSImageView {
        let icon = NSImageView(image: NSImage(systemSymbolName: name, accessibilityDescription: description)!)
        icon.contentTintColor = .secondaryLabelColor
        icon.symbolConfiguration = NSImage.SymbolConfiguration(pointSize: pointSize, weight: .regular)
        icon.setContentHuggingPriority(.required, for: .horizontal)
        return icon
    }

    private func makeStatusDot(color: NSColor) -> NSView {
        let dot = NSView()
        dot.wantsLayer = true
        dot.layer?.backgroundColor = color.cgColor
        dot.layer?.cornerRadius = CGFloat(PurePointTheme.statusDotSize) / 2
        dot.translatesAutoresizingMaskIntoConstraints = false
        let dotSize = CGFloat(PurePointTheme.statusDotSize)
        dot.widthAnchor.constraint(equalToConstant: dotSize).isActive = true
        dot.heightAnchor.constraint(equalToConstant: dotSize).isActive = true
        dot.setContentHuggingPriority(.required, for: .horizontal)
        return dot
    }

    private func makeBadge(count: Int) -> NSTextField {
        let badge = NSTextField(labelWithString: "\(count)")
        badge.font = .systemFont(ofSize: 10)
        badge.textColor = .secondaryLabelColor
        badge.alignment = .center
        badge.wantsLayer = true
        badge.layer?.backgroundColor = NSColor.quaternaryLabelColor.cgColor
        badge.layer?.cornerRadius = 6
        badge.setContentHuggingPriority(.required, for: .horizontal)
        let badgeWidth = max(18, badge.intrinsicContentSize.width + 8)
        badge.widthAnchor.constraint(equalToConstant: badgeWidth).isActive = true
        return badge
    }

    private func makeCountIcon(_ symbol: String, description: String, count: Int) -> NSStackView {
        let stack = NSStackView()
        stack.orientation = .horizontal
        stack.spacing = 2

        let icon = NSImageView(
            image: NSImage(systemSymbolName: symbol, accessibilityDescription: description)!)
        icon.contentTintColor = .tertiaryLabelColor
        icon.symbolConfiguration = NSImage.SymbolConfiguration(pointSize: 9, weight: .regular)
        icon.setContentHuggingPriority(.required, for: .horizontal)

        let label = NSTextField(labelWithString: "\(count)")
        label.font = .systemFont(ofSize: 10)
        label.textColor = .tertiaryLabelColor
        label.setContentHuggingPriority(.required, for: .horizontal)

        stack.addArrangedSubview(icon)
        stack.addArrangedSubview(label)
        stack.setContentHuggingPriority(.required, for: .horizontal)
        return stack
    }

    // MARK: - Helpers

    private func spacerView() -> NSView {
        let v = NSView()
        v.setContentHuggingPriority(.defaultLow, for: .horizontal)
        v.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        return v
    }

    private func makeInlineAddButton(action: Selector) -> NSButton {
        let button = NSButton()
        button.setButtonType(.momentaryPushIn)
        button.isBordered = false
        button.image = NSImage(systemSymbolName: "plus.circle", accessibilityDescription: "Add")
        button.imageScaling = .scaleProportionallyDown
        button.contentTintColor = .secondaryLabelColor
        button.target = self
        button.action = action
        button.setContentHuggingPriority(.required, for: .horizontal)
        button.translatesAutoresizingMaskIntoConstraints = false
        button.widthAnchor.constraint(equalToConstant: 16).isActive = true
        button.heightAnchor.constraint(equalToConstant: 16).isActive = true
        return button
    }

    private func makeMenuItem(title: String, action: Selector, context: Any? = nil) -> NSMenuItem {
        let item = NSMenuItem(title: title, action: action, keyEquivalent: "")
        item.target = self
        item.representedObject = context
        return item
    }

    // MARK: - Button Actions

    @objc private func projectAddClicked(_ sender: NSButton) {
        guard let projectRoot = sender.identifier?.rawValue,
            let project = findProject(byRoot: projectRoot)
        else { return }
        onShowCommandPalette?(project, nil, true)
    }

    @objc private func worktreeAddClicked(_ sender: NSButton) {
        guard let worktreeId = sender.identifier?.rawValue,
            let project = findProject(forWorktreeId: worktreeId)
        else { return }

        let ctx = WorktreeMenuContext(project: project, worktreeId: worktreeId)
        let menu = NSMenu()
        menu.addItem(makeMenuItem(title: "New Agent", action: #selector(menuNewAgentForWorktree(_:)), context: ctx))
        menu.addItem(
            makeMenuItem(title: "New Terminal", action: #selector(menuNewTerminalForWorktree(_:)), context: ctx))

        let point = NSPoint(x: 0, y: sender.bounds.height)
        menu.popUp(positioning: nil, at: point, in: sender)
    }

    @objc private func menuNewAgentForWorktree(_ sender: NSMenuItem) {
        guard let ctx = sender.representedObject as? WorktreeMenuContext else { return }
        onShowCommandPalette?(ctx.project, .worktree(ctx.worktreeId), false)
    }

    @objc private func menuNewTerminalForWorktree(_ sender: NSMenuItem) {
        guard let ctx = sender.representedObject as? WorktreeMenuContext else { return }
        guard let worktree = ctx.project.worktrees.first(where: { $0.id == ctx.worktreeId }) else { return }
        onAddTerminal?(ctx.project, worktree)
    }

    // MARK: - Project Lookup

    private func findProject(byRoot root: String) -> ProjectState? {
        for node in projectNodes {
            if case .project(let p) = node.kind, p.projectRoot == root { return p }
        }
        return nil
    }

    private func findProject(forWorktreeId wtId: String) -> ProjectState? {
        for node in projectNodes {
            if case .project(let p) = node.kind {
                if p.worktrees.contains(where: { $0.id == wtId }) { return p }
            }
        }
        return nil
    }

    private func findProject(forAgentId agentId: String) -> ProjectState? {
        for node in projectNodes {
            if case .project(let p) = node.kind {
                if p.agent(byId: agentId) != nil { return p }
            }
        }
        return nil
    }

    private func findProject(forWorkspaceId workspaceId: String) -> ProjectState? {
        for node in projectNodes {
            if case .project(let p) = node.kind,
                workspacesByProject[p.projectRoot]?.contains(where: { $0.id == workspaceId }) == true
            {
                return p
            }
        }
        return nil
    }

    private func findWorkspace(id workspaceId: String) -> Workspace? {
        for (_, list) in workspacesByProject {
            if let match = list.first(where: { $0.id == workspaceId }) { return match }
        }
        return nil
    }

    // MARK: - Inline Rename State

    private func cleanupEditingState() {
        InlineRenameFocus.isActive = false
        editingTextField = nil
        editingOriginalName = nil
        editingWorkspaceId = nil
    }
}

// MARK: - Context Menu

extension SidebarOutlineViewController: NSMenuDelegate {
    func menuNeedsUpdate(_ menu: NSMenu) {
        menu.removeAllItems()

        let clickedRow = outlineView.clickedRow
        guard clickedRow >= 0, let node = outlineView.item(atRow: clickedRow) as? SidebarNode else { return }
        contextClickedNode = node

        switch node.kind {
        case .workspace(let workspace): buildWorkspaceContextMenu(menu, workspace: workspace)
        case .worktree(let worktree): buildWorktreeContextMenu(menu, worktree: worktree)
        case .project(let project): buildProjectContextMenu(menu, project: project)
        }
    }

    private func buildWorkspaceContextMenu(_ menu: NSMenu, workspace: Workspace) {
        menu.addItem(makeMenuItem(title: "Rename\u{2026}", action: #selector(contextRenameAgent(_:))))
        menu.addItem(.separator())
        let agentCount = workspace.agentIds.count
        let title =
            switch agentCount {
            case 0: "Close Workspace"
            case 1 where workspace.tabCount == 1: "Kill Agent"
            case 1: "Close Workspace (1 agent)"
            default: "Close Workspace (\(agentCount) agents)"
            }
        menu.addItem(makeMenuItem(title: title, action: #selector(contextKillWorkspace(_:))))
    }

    private func buildWorktreeContextMenu(_ menu: NSMenu, worktree: WorktreeModel) {
        let aliveCount = worktree.agents.filter { $0.status.isAlive }.count
        if aliveCount > 0 {
            menu.addItem(
                makeMenuItem(
                    title: "Kill All Agents (\(aliveCount))",
                    action: #selector(contextKillWorktreeAgents(_:))
                ))
            menu.addItem(.separator())
        }
        menu.addItem(makeMenuItem(title: "Delete Worktree\u{2026}", action: #selector(contextDeleteWorktree(_:))))
    }

    private func buildProjectContextMenu(_ menu: NSMenu, project: ProjectState) {
        let aliveCount = project.allAgents.filter { $0.status.isAlive }.count
        if aliveCount > 0 {
            menu.addItem(
                makeMenuItem(
                    title: "Kill All Agents (\(aliveCount))",
                    action: #selector(contextKillAllProjectAgents(_:))
                ))
            menu.addItem(.separator())
        }
        menu.addItem(makeMenuItem(title: "Remove Project\u{2026}", action: #selector(contextRemoveProject(_:))))
    }

    @objc private func contextKillWorkspace(_ sender: NSMenuItem) {
        guard let node = contextClickedNode, case .workspace(let workspace) = node.kind else { return }
        guard let project = findProject(forWorkspaceId: workspace.id) else { return }

        let agentCount = workspace.agentIds.count
        guard agentCount > 1 else {
            onKillWorkspace?(project, workspace.id)
            return
        }
        showConfirmation(
            title: "Close this workspace?",
            message: "This will kill \(agentCount) agents across its panes.",
            confirmTitle: "Close"
        ) {
            self.onKillWorkspace?(project, workspace.id)
        }
    }

    @objc private func contextKillWorktreeAgents(_ sender: NSMenuItem) {
        guard let node = contextClickedNode, case .worktree(let worktree) = node.kind else { return }
        guard let project = findProject(forWorktreeId: worktree.id) else { return }
        let aliveCount = worktree.agents.filter { $0.status.isAlive }.count
        showConfirmation(
            title: "Kill All Agents in \(worktree.branch)?",
            message: "This will kill \(aliveCount) running agent\(aliveCount == 1 ? "" : "s") in this worktree."
        ) {
            self.onKillWorktreeAgents?(project, worktree.id)
        }
    }

    @objc private func contextDeleteWorktree(_ sender: NSMenuItem) {
        guard let node = contextClickedNode, case .worktree(let worktree) = node.kind else { return }
        guard let project = findProject(forWorktreeId: worktree.id) else { return }
        let aliveCount = worktree.agents.filter { $0.status.isAlive }.count
        let agentNote =
            aliveCount > 0
            ? "This will kill \(aliveCount) running agent\(aliveCount == 1 ? "" : "s"), "
            : "This will "
        showConfirmation(
            title: "Delete worktree \(worktree.branch)?",
            message: "\(agentNote)remove the worktree directory, and delete the branch locally and from GitHub.",
            confirmTitle: "Delete"
        ) {
            self.onDeleteWorktree?(project, worktree.id)
        }
    }

    @objc private func contextKillAllProjectAgents(_ sender: NSMenuItem) {
        guard let node = contextClickedNode, case .project(let project) = node.kind else { return }
        let aliveCount = project.allAgents.filter { $0.status.isAlive }.count
        showConfirmation(
            title: "Kill All Agents in \(project.projectName)?",
            message: "This will kill \(aliveCount) running agent\(aliveCount == 1 ? "" : "s") in this project."
        ) {
            self.onKillAllProjectAgents?(project)
        }
    }

    @objc private func contextRemoveProject(_ sender: NSMenuItem) {
        guard let node = contextClickedNode, case .project(let project) = node.kind else { return }
        showConfirmation(
            title: "Remove \(project.projectName)?",
            message:
                "This will close the project in PurePoint and delete .pu/manifest.json and .pu/agents/. "
                + "The project folder will not be deleted.",
            confirmTitle: "Remove"
        ) {
            self.onRemoveProject?(project)
        }
    }

    private static let workspaceNameLabelId = NSUserInterfaceItemIdentifier("workspaceNameLabel")

    @objc private func contextRenameAgent(_ sender: NSMenuItem) {
        guard let node = contextClickedNode, case .workspace(let workspace) = node.kind else { return }
        let clickedRow = outlineView.row(forItem: node)
        guard clickedRow >= 0,
            let cellView = outlineView.view(atColumn: 0, row: clickedRow, makeIfNecessary: false),
            let textField = findNameTextField(in: cellView)
        else { return }

        InlineRenameFocus.isActive = true
        editingWorkspaceId = workspace.id
        editingOriginalName = textField.stringValue
        editingTextField = textField

        textField.isEditable = true
        textField.isSelectable = true
        textField.isBezeled = false
        textField.usesSingleLineMode = true
        textField.focusRingType = .none
        textField.delegate = self

        // Defer so the context menu's focus-restoration teardown completes first.
        DispatchQueue.main.async { [weak self] in
            self?.beginFieldEditing(textField, retriesLeft: 2)
        }
    }

    /// Make the field first responder and confirm a field editor is actually attached;
    /// if something stole focus in the meantime, try again on the next tick.
    private func beginFieldEditing(_ textField: NSTextField, retriesLeft: Int) {
        guard editingTextField === textField, let window = view.window else { return }
        // becomeFirstResponder attaches the field editor and selects all text. Don't follow it
        // with selectText(nil): on a field already editing, that ends editing first, and
        // controlTextDidEndEditing would then leave the field non-editable (dead input).
        isStartingRename = true
        window.makeFirstResponder(textField)
        isStartingRename = false
        if textField.currentEditor() == nil, retriesLeft > 0 {
            DispatchQueue.main.async { [weak self] in
                self?.beginFieldEditing(textField, retriesLeft: retriesLeft - 1)
            }
        }
    }

    /// Find the name label text field in a cell view (the non-dot, non-icon label).
    private func findNameTextField(in cellView: NSView) -> NSTextField? {
        for subview in cellView.subviews {
            if let stack = subview as? NSStackView {
                for arranged in stack.arrangedSubviews {
                    if let tf = arranged as? NSTextField, tf.identifier == Self.workspaceNameLabelId {
                        return tf
                    }
                }
            }
        }
        return nil
    }

    // MARK: - Confirmation Dialog

    private func showConfirmation(
        title: String, message: String, confirmTitle: String = "Kill All", action: @escaping () -> Void
    ) {
        let alert = NSAlert()
        alert.messageText = title
        alert.informativeText = message
        alert.alertStyle = .warning
        alert.addButton(withTitle: confirmTitle)
        alert.addButton(withTitle: "Cancel")

        if let window = view.window {
            alert.beginSheetModal(for: window) { response in
                if response == .alertFirstButtonReturn {
                    action()
                }
            }
        } else {
            let response = alert.runModal()
            if response == .alertFirstButtonReturn {
                action()
            }
        }
    }
}

// MARK: - Inline Rename (NSTextFieldDelegate)

extension SidebarOutlineViewController: NSTextFieldDelegate {
    func control(_ control: NSControl, textView: NSTextView, doCommandBy commandSelector: Selector) -> Bool {
        if commandSelector == #selector(NSResponder.cancelOperation(_:)) {
            if let tf = editingTextField, let original = editingOriginalName {
                tf.stringValue = original
                tf.isEditable = false
                tf.isSelectable = false
            }
            cleanupEditingState()
            view.window?.makeFirstResponder(outlineView)
            return true
        }
        return false
    }

    func controlTextDidEndEditing(_ obj: Notification) {
        guard !isStartingRename else { return }
        guard let tf = editingTextField, let workspaceId = editingWorkspaceId else { return }
        let newName = tf.stringValue.trimmingCharacters(in: .whitespaces)

        tf.isEditable = false
        tf.isSelectable = false

        // Renaming a workspace renames the agent it is named after.
        if !newName.isEmpty, newName != editingOriginalName,
            let agentId = findWorkspace(id: workspaceId)?.primaryAgentId,
            let project = findProject(forWorkspaceId: workspaceId)
        {
            onRenameAgent?(project, agentId, newName)
        } else if let original = editingOriginalName {
            tf.stringValue = original
        }

        cleanupEditingState()
    }
}

/// Set while a sidebar row is being renamed so terminals don't take first responder
/// away from the field editor when the selection switches content.
enum InlineRenameFocus {
    static var isActive = false
}

// MARK: - WorktreeMenuContext

private class WorktreeMenuContext: NSObject {
    let project: ProjectState
    let worktreeId: String
    init(project: ProjectState, worktreeId: String) {
        self.project = project
        self.worktreeId = worktreeId
    }
}
