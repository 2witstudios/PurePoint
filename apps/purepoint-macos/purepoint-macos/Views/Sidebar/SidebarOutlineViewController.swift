import AppKit

/// NSViewController hosting an NSOutlineView for compact sidebar rows.
/// Combines data source, delegate, and cell factories.
@MainActor
class SidebarOutlineViewController: NSViewController, NSOutlineViewDataSource, NSOutlineViewDelegate {

    let scrollView = NSScrollView()
    let outlineView = NSOutlineView()

    var projectNodes: [SidebarNode] = []

    /// Callback when user clicks a row — maps to SidebarSelection.
    var onSelectionChanged: ((SidebarSelection?) -> Void)?

    /// Callback for showing the command palette for a project+selection context.
    var onShowCommandPalette: ((ProjectState, SidebarSelection?, Bool) -> Void)?

    /// Callback for creating a terminal in a worktree.
    var onAddTerminal: ((ProjectState, WorktreeModel) -> Void)?

    /// Callback for killing an agent.
    var onKillAgent: ((ProjectState, String) -> Void)?

    /// Callback for killing all agents in a worktree.
    var onKillWorktreeAgents: ((ProjectState, String) -> Void)?

    /// Agent currently shown in the grid.
    var gridOwnerAgentId: String?

    /// Agent IDs hidden from the sidebar (grid children).
    var hiddenAgentIds: Set<String> = []

    /// Project root for grid filtering.
    var gridProjectRoot: String?

    /// Prevents feedback loops during programmatic selection changes.
    private var suppressSelectionCallback = false

    /// Node for context-menu tracking.
    private var contextClickedNode: SidebarNode?

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
        let oldExpanded = expandedNodeIds()
        let oldSelectedId = selectedNodeId()

        var newNodes: [SidebarNode] = []
        for project in projects {
            let isGridProject = project.projectRoot == gridProjectRoot

            // Root agents (filtered for grid children)
            let rootAgents: [AgentModel]
            if isGridProject {
                rootAgents = project.rootAgents.filter { !hiddenAgentIds.contains($0.id) }
            } else {
                rootAgents = project.rootAgents
            }

            var projectChildren: [SidebarNode] = rootAgents.map {
                SidebarNode(kind: .agent($0))
            }

            for worktree in project.worktrees {
                let agents: [AgentModel]
                if isGridProject {
                    agents = worktree.agents.filter { !hiddenAgentIds.contains($0.id) }
                } else {
                    agents = worktree.agents
                }
                let agentNodes = agents.map { SidebarNode(kind: .agent($0)) }
                let wtNode = SidebarNode(kind: .worktree(worktree), children: agentNodes)
                projectChildren.append(wtNode)
            }

            let projectNode = SidebarNode(kind: .project(project), children: projectChildren)
            newNodes.append(projectNode)
        }

        projectNodes = newNodes
        outlineView.reloadData()

        // Restore expansion — auto-expand new items
        for node in projectNodes {
            let shouldExpand = oldExpanded.contains(node.id) || !oldExpanded.contains(where: { _ in true }) || !oldExpanded.contains(node.id) && isNew(node.id, in: oldExpanded, allOldIds: oldExpanded)
            outlineView.expandItem(node) // Always expand projects
            for child in node.children {
                if case .worktree = child.kind {
                    if oldExpanded.contains(child.id) || !oldExpanded.contains(child.id) && isNewWorktree(child.id, oldExpanded: oldExpanded) {
                        outlineView.expandItem(child)
                    }
                }
            }
        }

        // Restore selection
        if let oldSelectedId {
            selectNode(withId: oldSelectedId)
        }
    }

    private func isNew(_ id: String, in expanded: Set<String>, allOldIds: Set<String>) -> Bool {
        !allOldIds.contains(id)
    }

    private func isNewWorktree(_ id: String, oldExpanded: Set<String>) -> Bool {
        // If it wasn't in the old set at all, it's new — auto-expand
        !oldExpanded.contains(id)
    }

    private func expandedNodeIds() -> Set<String> {
        var ids = Set<String>()
        for node in projectNodes {
            if outlineView.isItemExpanded(node) { ids.insert(node.id) }
            for child in node.children {
                if outlineView.isItemExpanded(child) { ids.insert(child.id) }
            }
        }
        return ids
    }

    private func selectedNodeId() -> String? {
        let row = outlineView.selectedRow
        guard row >= 0, let node = outlineView.item(atRow: row) as? SidebarNode else { return nil }
        return node.id
    }

    // MARK: - Programmatic Selection

    func selectNode(for selection: SidebarSelection?) {
        guard let selection else {
            suppressSelectionCallback = true
            outlineView.deselectAll(nil)
            suppressSelectionCallback = false
            return
        }

        let targetId: String?
        switch selection {
        case .agent(let id): targetId = id
        case .worktree(let id): targetId = id
        case .project(let root): targetId = root
        case .nav, .terminal: targetId = nil
        }

        guard let targetId else { return }
        selectNode(withId: targetId)
    }

    private func selectNode(withId targetId: String) {
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
        case .agent: return false
        }
    }

    // MARK: - NSOutlineViewDelegate

    func outlineView(_ outlineView: NSOutlineView, viewFor tableColumn: NSTableColumn?, item: Any) -> NSView? {
        guard let node = item as? SidebarNode else { return nil }
        switch node.kind {
        case .project(let project): return makeProjectCell(project)
        case .worktree(let worktree): return makeWorktreeCell(worktree, node: node)
        case .agent(let agent): return makeAgentCell(agent)
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
        case .agent(let a): selection = .agent(a.id)
        }
        onSelectionChanged?(selection)
    }

    // MARK: - Cell Factories

    private func makeProjectCell(_ project: ProjectState) -> NSView {
        let cell = NSTableCellView()
        let stack = NSStackView()
        stack.orientation = .horizontal
        stack.spacing = 6
        stack.translatesAutoresizingMaskIntoConstraints = false

        let icon = NSImageView(image: NSImage(systemSymbolName: "folder.fill", accessibilityDescription: "Project")!)
        icon.contentTintColor = .secondaryLabelColor
        icon.symbolConfiguration = NSImage.SymbolConfiguration(pointSize: 11, weight: .regular)
        icon.setContentHuggingPriority(.required, for: .horizontal)

        let name = NSTextField(labelWithString: project.projectName)
        name.font = .systemFont(ofSize: 12, weight: .semibold)
        name.lineBreakMode = .byTruncatingTail

        let addBtn = makeInlineAddButton(action: #selector(projectAddClicked(_:)))
        addBtn.identifier = NSUserInterfaceItemIdentifier(project.projectRoot)

        stack.addArrangedSubview(icon)
        stack.addArrangedSubview(name)
        stack.addArrangedSubview(spacerView())
        stack.addArrangedSubview(addBtn)

        cell.addSubview(stack)
        NSLayoutConstraint.activate([
            stack.leadingAnchor.constraint(equalTo: cell.leadingAnchor, constant: 2),
            stack.trailingAnchor.constraint(equalTo: cell.trailingAnchor, constant: -4),
            stack.centerYAnchor.constraint(equalTo: cell.centerYAnchor),
        ])
        return cell
    }

    private func makeWorktreeCell(_ worktree: WorktreeModel, node: SidebarNode) -> NSView {
        let cell = NSTableCellView()
        let stack = NSStackView()
        stack.orientation = .horizontal
        stack.spacing = 6
        stack.translatesAutoresizingMaskIntoConstraints = false

        let icon = NSImageView(image: NSImage(systemSymbolName: "arrow.triangle.branch", accessibilityDescription: "Worktree")!)
        icon.contentTintColor = .secondaryLabelColor
        icon.symbolConfiguration = NSImage.SymbolConfiguration(pointSize: 10, weight: .regular)
        icon.setContentHuggingPriority(.required, for: .horizontal)

        let name = NSTextField(labelWithString: worktree.branch)
        name.font = .systemFont(ofSize: 12)
        name.lineBreakMode = .byTruncatingTail

        let addBtn = makeInlineAddButton(action: #selector(worktreeAddClicked(_:)))
        addBtn.identifier = NSUserInterfaceItemIdentifier(worktree.id)

        stack.addArrangedSubview(icon)
        stack.addArrangedSubview(name)
        stack.addArrangedSubview(spacerView())
        stack.addArrangedSubview(addBtn)

        // Agent count badge
        let agentCount = worktree.agents.count
        if agentCount > 0 {
            let badge = NSTextField(labelWithString: "\(agentCount)")
            badge.font = .systemFont(ofSize: 10)
            badge.textColor = .secondaryLabelColor
            badge.alignment = .center
            badge.wantsLayer = true
            badge.layer?.backgroundColor = NSColor.quaternaryLabelColor.cgColor
            badge.layer?.cornerRadius = 6
            badge.setContentHuggingPriority(.required, for: .horizontal)
            let badgeWidth = max(18, badge.intrinsicContentSize.width + 8)
            badge.widthAnchor.constraint(equalToConstant: badgeWidth).isActive = true
            stack.addArrangedSubview(badge)
        }

        cell.addSubview(stack)
        NSLayoutConstraint.activate([
            stack.leadingAnchor.constraint(equalTo: cell.leadingAnchor, constant: 2),
            stack.trailingAnchor.constraint(equalTo: cell.trailingAnchor, constant: -4),
            stack.centerYAnchor.constraint(equalTo: cell.centerYAnchor),
        ])
        return cell
    }

    private func makeAgentCell(_ agent: AgentModel) -> NSView {
        let cell = NSTableCellView()
        let stack = NSStackView()
        stack.orientation = .horizontal
        stack.spacing = 5
        stack.translatesAutoresizingMaskIntoConstraints = false

        // Status dot
        let dot = NSView()
        dot.wantsLayer = true
        dot.layer?.backgroundColor = agent.status.nsColor.cgColor
        dot.layer?.cornerRadius = CGFloat(PurePointTheme.statusDotSize) / 2
        dot.translatesAutoresizingMaskIntoConstraints = false
        let dotSize = CGFloat(PurePointTheme.statusDotSize)
        dot.widthAnchor.constraint(equalToConstant: dotSize).isActive = true
        dot.heightAnchor.constraint(equalToConstant: dotSize).isActive = true
        dot.setContentHuggingPriority(.required, for: .horizontal)

        let label = NSTextField(labelWithString: agent.displayName)
        label.font = .systemFont(ofSize: 11)
        label.lineBreakMode = .byTruncatingTail

        stack.addArrangedSubview(dot)
        stack.addArrangedSubview(label)

        // Grid owner icon
        if agent.id == gridOwnerAgentId {
            let gridIcon = NSImageView(image: NSImage(systemSymbolName: "rectangle.split.2x2", accessibilityDescription: "Grid owner")!)
            gridIcon.contentTintColor = .tertiaryLabelColor
            gridIcon.symbolConfiguration = NSImage.SymbolConfiguration(pointSize: 9, weight: .regular)
            gridIcon.setContentHuggingPriority(.required, for: .horizontal)
            stack.addArrangedSubview(gridIcon)
        }

        cell.addSubview(stack)
        NSLayoutConstraint.activate([
            stack.leadingAnchor.constraint(equalTo: cell.leadingAnchor, constant: 2),
            stack.trailingAnchor.constraint(equalTo: cell.trailingAnchor, constant: -4),
            stack.centerYAnchor.constraint(equalTo: cell.centerYAnchor),
        ])

        cell.setAccessibilityLabel("\(agent.displayName), \(agent.status.rawValue)")
        return cell
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

    // MARK: - Button Actions

    @objc private func projectAddClicked(_ sender: NSButton) {
        guard let projectRoot = sender.identifier?.rawValue else { return }
        guard let project = findProject(byRoot: projectRoot) else { return }
        onShowCommandPalette?(project, nil, true)
    }

    @objc private func worktreeAddClicked(_ sender: NSButton) {
        guard let worktreeId = sender.identifier?.rawValue else { return }
        guard let project = findProject(forWorktreeId: worktreeId) else { return }

        let menu = NSMenu()

        let agentItem = NSMenuItem(title: "New Agent", action: #selector(menuNewAgentForWorktree(_:)), keyEquivalent: "")
        agentItem.target = self
        agentItem.representedObject = WorktreeMenuContext(project: project, worktreeId: worktreeId)
        menu.addItem(agentItem)

        let termItem = NSMenuItem(title: "New Terminal", action: #selector(menuNewTerminalForWorktree(_:)), keyEquivalent: "")
        termItem.target = self
        termItem.representedObject = WorktreeMenuContext(project: project, worktreeId: worktreeId)
        menu.addItem(termItem)

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
}

// MARK: - Context Menu

extension SidebarOutlineViewController: NSMenuDelegate {
    func menuNeedsUpdate(_ menu: NSMenu) {
        menu.removeAllItems()

        let clickedRow = outlineView.clickedRow
        guard clickedRow >= 0, let node = outlineView.item(atRow: clickedRow) as? SidebarNode else { return }
        contextClickedNode = node

        switch node.kind {
        case .agent(let agent):
            let killItem = NSMenuItem(title: "Kill Agent", action: #selector(contextKillAgent(_:)), keyEquivalent: "")
            killItem.target = self
            menu.addItem(killItem)

        case .worktree(let worktree):
            guard worktree.agents.count > 0 else { return }
            let killAllItem = NSMenuItem(title: "Kill All Agents", action: #selector(contextKillWorktreeAgents(_:)), keyEquivalent: "")
            killAllItem.target = self
            menu.addItem(killAllItem)

        case .project:
            break
        }
    }

    @objc private func contextKillAgent(_ sender: NSMenuItem) {
        guard let node = contextClickedNode, case .agent(let agent) = node.kind else { return }
        guard let project = findProject(forAgentId: agent.id) else { return }
        onKillAgent?(project, agent.id)
    }

    @objc private func contextKillWorktreeAgents(_ sender: NSMenuItem) {
        guard let node = contextClickedNode, case .worktree(let worktree) = node.kind else { return }
        guard let project = findProject(forWorktreeId: worktree.id) else { return }
        onKillWorktreeAgents?(project, worktree.id)
    }
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
