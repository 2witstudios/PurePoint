#if os(macOS)
import AppKit

/// NSViewController hosting an NSOutlineView for compact sidebar rows.
/// Combines data source, delegate, and cell factories.
@MainActor
class SidebarOutlineViewController: NSViewController, NSOutlineViewDataSource, NSOutlineViewDelegate {

    let scrollView = NSScrollView()
    let outlineView = SidebarKeyOutlineView()

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

    /// Callback for forgetting a machine (its forward closes; nothing on it changes).
    var onRemoveMachine: ((MachineState) -> Void)?

    /// Canonical workspaces per project root. The sidebar renders these and nothing else —
    /// there is no agent list to filter, so a pane cannot leak out as its own row.
    var workspacesByProject: [String: [Workspace]] = [:]

    /// The workspace whose grid is on screen, for the active-row marker.
    var activeWorkspaceId: String?

    /// Prevents feedback loops during programmatic selection changes.
    private var suppressSelectionCallback = false

    /// Node for context-menu tracking.
    private var contextClickedNode: SidebarNode?

    /// Which rows the user closed. AppKit forgets expansion whenever it reloads, and every
    /// rebuild reloads, so the answer is kept here and replayed after each one.
    private let expansionStore: SidebarExpansionStore

    /// Set while the recorded state is replayed, so the notifications a real expand or
    /// collapse posts are not taken for the user's.
    private var isRestoringExpansion = false

    /// Inline rename state.
    private var editingTextField: NSTextField?
    private var isStartingRename = false
    private var editingOriginalName: String?
    private var editingWorkspaceId: String?

    init(expansionStore: SidebarExpansionStore) {
        self.expansionStore = expansionStore
        super.init(nibName: nil, bundle: nil)
    }

    required init?(coder: NSCoder) {
        fatalError("init(coder:) is not supported: the sidebar is built in code")
    }

    // MARK: - Lifecycle

    override func loadView() {
        view = NSView()
    }

    override func viewDidLoad() {
        super.viewDidLoad()
        outlineView.onReturn = { [weak self] in self?.toggleSelectedMachineConnection() ?? false }

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

    /// Rebuild the node tree from AppState projects, then a MACHINES section with each
    /// machine's projects nested under it.
    func rebuildNodes(projects: [ProjectState], machines: [MachineState] = []) {
        // Reloading mid-rename discards the cell holding the field editor, leaving it dead.
        // `lastRenderState` is untouched, so the next update after the edit rebuilds.
        guard editingTextField == nil else { return }
        let nextRenderState = SidebarRenderState.make(
            projects: projects, machines: machines, workspacesByProject: workspacesByProject,
            activeWorkspaceId: activeWorkspaceId)
        guard nextRenderState != lastRenderState else { return }

        let oldSelectedId = selectedNodeId()
        let scrollOrigin = scrollView.contentView.bounds.origin

        projectNodes = SidebarTree.build(projects: projects, machines: machines, workspacesByProject: workspacesByProject)
        lastRenderState = nextRenderState
        // Reloading drops every expansion AppKit knows about, and it may announce that as it
        // does. Neither that nor the replay below is the user opening or closing a row.
        isRestoringExpansion = true
        outlineView.reloadData()
        restoreExpansionState()
        isRestoringExpansion = false

        if let oldSelectedId {
            selectNode(withId: oldSelectedId)
        }

        scrollView.contentView.scroll(to: scrollOrigin)
        scrollView.reflectScrolledClipView(scrollView.contentView)
    }

    /// Close the rows the user closed and leave the rest open. `reloadData()` above leaves
    /// every row collapsed, so the recorded rows stay collapsed and everything else is
    /// expanded.
    private func restoreExpansionState() {
        let liveClosed = applyExpansionState(projectNodes, parentOpen: true)
        // Rows that no longer exist are dropped here, so a removed row cannot resurrect
        // itself when it comes back. Until the tree is settled, though, a row with nothing
        // under it has simply not loaded yet, and recording that as the whole tree would
        // erase the saved state of rows that have not arrived — a project's worktrees load
        // after its row, a machine's projects after its inventory answers.
        guard isTreeSettled else { return }
        expansionStore.record(liveClosed)
    }

    /// Whether every row in the tree holds all the rows it will ever hold.
    /// See `SidebarTree.isSettled(_:)`.
    private var isTreeSettled: Bool { SidebarTree.isSettled(projectNodes) }

    /// Expands the rows that are not recorded closed and reports the ones in the tree that
    /// are, parents before children.
    private func applyExpansionState(_ nodes: [SidebarNode], parentOpen: Bool) -> Set<String> {
        let expansion = SidebarTree.expansion(
            nodes, closedRowKeys: expansionStore.closedRowKeys, parentOpen: parentOpen)
        for node in expansion.open {
            outlineView.expandItem(node)
        }
        return expansion.closed
    }

    private func selectedNodeId() -> String? {
        let row = outlineView.selectedRow
        guard row >= 0, let node = outlineView.item(atRow: row) as? SidebarNode else { return nil }
        return node.id
    }

    // MARK: - Programmatic Selection

    func selectNode(for selection: SidebarSelection?) {
        guard let targetId = SidebarTree.targetId(for: selection) else {
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

        // A row inside a closed row has no row of its own yet, so its ancestors are opened
        // first — otherwise selecting a workspace inside a collapsed project would silently
        // find nothing.
        for ancestor in ancestors(where: { $0.id == targetId }) {
            outlineView.expandItem(ancestor)
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

    /// The rows above a matching row, outermost first. Empty when the row is at the top level
    /// or is not in the tree.
    private func ancestors(where matches: (SidebarNode) -> Bool) -> [SidebarNode] {
        SidebarTree.ancestors(where: matches, in: projectNodes)
    }

    // MARK: - Row Expansion

    func outlineViewItemDidExpand(_ notification: Notification) {
        // While the recorded state is replayed the walk opens what it needs itself; the
        // cascade below would only open the same rows twice.
        guard !isRestoringExpansion,
            let node = notification.userInfo?["NSObject"] as? SidebarNode
        else { return }

        openRows(under: node)
    }

    func outlineViewItemWillCollapse(_ notification: Notification) {
        // The whole tree is reloaded on every rebuild, and AppKit drops every expansion
        // with it; that is not the user closing anything.
        guard !isRestoringExpansion,
            let node = notification.userInfo?["NSObject"] as? SidebarNode
        else { return }

        // Closing a row also announces every open row inside it, because those rows lose
        // their place on screen. They were not closed: recording them would leave them
        // closed when the parent is opened again.
        guard !isInsideClosedRow(node) else { return }
        expansionStore.setExpanded(node.rowKey, isExpanded: false)
    }

    /// Whether some row above this one is already closed.
    ///
    /// AppKit announces a row's open descendants *after* it has collapsed the row itself, so
    /// an ancestor that already reads collapsed means this announcement is part of that
    /// cascade rather than the user's own. (The row being closed still reads expanded in its
    /// own notification, which is why only the ancestors are inspected.)
    private func isInsideClosedRow(_ node: SidebarNode) -> Bool {
        // Identity, not id: the notification carries the very node instance the outline view
        // holds, and two rows can share an id.
        ancestors { $0 === node }.contains { !outlineView.isItemExpanded($0) }
    }

    /// Opens a row the user just opened and the rows under it with it. AppKit expands only
    /// the row itself, so without this every row beneath a reopened row would come back
    /// closed until it is opened again.
    private func openRows(under parent: SidebarNode) {
        expansionStore.setExpanded(parent.rowKey, isExpanded: true)
        for child in parent.children where child.isExpandable {
            guard !expansionStore.closedRowKeys.contains(child.rowKey) else { continue }
            outlineView.expandItem(child)
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
        return node.isExpandable
    }

    func outlineView(_ outlineView: NSOutlineView, shouldSelectItem item: Any) -> Bool {
        guard let node = item as? SidebarNode else { return false }
        return SidebarTree.isSelectable(node)
    }

    // MARK: - NSOutlineViewDelegate

    func outlineView(_ outlineView: NSOutlineView, viewFor tableColumn: NSTableColumn?, item: Any) -> NSView? {
        guard let node = item as? SidebarNode else { return nil }
        switch node.kind {
        case .project(let project): return makeProjectCell(project)
        case .worktree(let worktree): return makeWorktreeCell(worktree, node: node)
        case .workspace(let workspace): return makeWorkspaceCell(workspace, node: node)
        case .sectionHeader(let title): return makeSectionHeaderCell(title)
        case .machine(let machine): return makeMachineCell(machine)
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

        guard let selection = SidebarTree.selection(for: node) else { return }
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
        addBtn.identifier = NSUserInterfaceItemIdentifier(project.projectId)

        stack.addArrangedSubview(icon)
        stack.addArrangedSubview(name)
        stack.addArrangedSubview(spacerView())
        if project.isUnavailable {
            // Kept only for its unsaved file tabs while the machine's daemon is unreachable.
            name.textColor = .secondaryLabelColor
            let status = NSTextField(labelWithString: "Unavailable")
            status.font = .systemFont(ofSize: 10)
            status.textColor = .secondaryLabelColor
            stack.addArrangedSubview(status)
            cell.toolTip = "The machine's daemon is unreachable. Unsaved file tabs are kept until it is back."
            cell.setAccessibilityLabel("\(project.projectName), unavailable")
        } else if project.isRemote, let error = project.registry?.layoutErrorsByProject[project.projectId] {
            let warning = makeSymbolIcon("exclamationmark.triangle.fill", description: "Layout error", pointSize: 10)
            warning.contentTintColor = .systemOrange
            stack.addArrangedSubview(warning)
            cell.toolTip = error + " Retrying; choose Retry Layout to try now."
            cell.setAccessibilityLabel("\(project.projectName), \(error)")
        }
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

    private func makeSectionHeaderCell(_ title: String) -> NSView {
        let (cell, stack) = makeCellWithStack(spacing: 6)
        let label = NSTextField(labelWithString: title)
        label.font = .systemFont(ofSize: 10, weight: .semibold)
        label.textColor = .secondaryLabelColor
        stack.addArrangedSubview(label)
        stack.addArrangedSubview(spacerView())
        cell.setAccessibilityLabel("Machines")
        return cell
    }

    private func makeMachineCell(_ machine: MachineState) -> NSView {
        let (cell, stack) = makeCellWithStack(spacing: 6)

        stack.addArrangedSubview(makeStatusDot(color: Self.statusColor(machine.state)))
        let icon = makeSymbolIcon("desktopcomputer", description: "Machine", pointSize: 11)
        let name = NSTextField(labelWithString: machine.machine.name)
        name.font = .systemFont(ofSize: 12, weight: .semibold)
        name.lineBreakMode = .byTruncatingTail
        let status = NSTextField(labelWithString: machine.statusText)
        status.font = .systemFont(ofSize: 10)
        status.textColor = .secondaryLabelColor
        status.lineBreakMode = .byTruncatingTail
        status.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)

        stack.addArrangedSubview(icon)
        stack.addArrangedSubview(name)
        stack.addArrangedSubview(spacerView())
        stack.addArrangedSubview(status)
        var tooltip = "\(machine.machine.name) (\(machine.machine.sshHost)): \(machine.statusText)"
        if case .offline(let reason) = machine.state { tooltip += " — " + reason }
        if let error = machine.actionError {
            let warning = makeSymbolIcon("exclamationmark.triangle.fill", description: "Error", pointSize: 10)
            warning.contentTintColor = .systemOrange
            stack.addArrangedSubview(warning)
            tooltip += "\n" + error
        }
        cell.toolTip = tooltip
        cell.setAccessibilityLabel(tooltip)
        cell.setAccessibilityCustomActions(machineAccessibilityActions(machine))
        return cell
    }

    /// The machine's menu actions, reachable from VoiceOver's actions rotor.
    private func machineAccessibilityActions(_ machine: MachineState) -> [NSAccessibilityCustomAction] {
        var actions: [NSAccessibilityCustomAction] = []
        if machine.isConnectedOrConnecting {
            actions.append(
                NSAccessibilityCustomAction(name: "Disconnect") { [weak self, weak machine] in
                    guard let self, let machine else { return false }
                    self.confirmDisconnect(machine)
                    return true
                })
        } else {
            actions.append(
                NSAccessibilityCustomAction(name: "Connect") { [weak machine] in
                    machine?.connect()
                    return machine != nil
                })
        }
        if machine.state == .connected(.stopped), !machine.isStartingDaemon {
            actions.append(
                NSAccessibilityCustomAction(name: "Start Daemon") { [weak machine] in
                    machine?.startDaemon()
                    return machine != nil
                })
        }
        if machine.hiddenProjectCount > 0 {
            actions.append(
                NSAccessibilityCustomAction(name: "Show Hidden Projects") { [weak machine] in
                    machine?.showHiddenProjects()
                    return machine != nil
                })
        }
        if machine.actionError != nil {
            actions.append(
                NSAccessibilityCustomAction(name: "Dismiss Error") { [weak machine] in
                    machine?.actionError = nil
                    return machine != nil
                })
        }
        actions.append(
            NSAccessibilityCustomAction(name: "Remove Machine") { [weak self, weak machine] in
                guard let self, let machine else { return false }
                self.confirmRemove(machine)
                return true
            })
        return actions
    }

    private static func statusColor(_ state: MachineConnectionState) -> NSColor {
        switch state {
        case .connected(.running): .systemGreen
        case .connected(.stopped): .systemOrange
        case .connected(.incompatible): .systemRed
        case .connecting: .systemYellow
        case .offline: .systemRed
        case .disconnected: .tertiaryLabelColor
        }
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
        let state = SidebarRenderState.workspace(workspace, in: project)

        stack.addArrangedSubview(makeStatusDot(color: state.status?.nsColor ?? .tertiaryLabelColor))

        let label = NSTextField(labelWithString: state.title)
        label.font = .systemFont(ofSize: 11)
        label.lineBreakMode = .byTruncatingTail
        label.setContentHuggingPriority(.defaultHigh, for: .horizontal)
        label.identifier = Self.workspaceNameLabelId
        label.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        stack.addArrangedSubview(label)
        if let signal = state.signal {
            let pill = NSTextField(labelWithString: " \(signal.label) ")
            pill.font = .systemFont(ofSize: 9, weight: .medium)
            pill.textColor = signal.needsAttention ? .labelColor : .secondaryLabelColor
            pill.wantsLayer = true
            pill.layer?.cornerRadius = 4
            pill.layer?.backgroundColor = NSColor.quaternaryLabelColor.cgColor
            pill.setContentCompressionResistancePriority(.required, for: .horizontal)
            pill.setContentHuggingPriority(.required, for: .horizontal)
            stack.addArrangedSubview(pill)
        }

        stack.addArrangedSubview(spacerView())
        if state.paneCount > 1 {
            stack.addArrangedSubview(makeCountIcon("rectangle.split.2x2", description: "Panes", count: state.paneCount))
        }
        if state.tabCount > state.paneCount {
            stack.addArrangedSubview(makeCountIcon("square.stack", description: "Tabs", count: state.tabCount))
        }

        let paneNote = state.paneCount > 1 ? ", \(state.paneCount) panes" : ""
        let tabNote = state.tabCount > state.paneCount ? ", \(state.tabCount) tabs" : ""
        cell.setAccessibilityLabel("\(state.title), \(state.signal?.accessibilityDescription ?? state.status?.rawValue ?? "files")\(paneNote)\(tabNote)")
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

    /// Local projects and every machine's projects, in sidebar order.
    private var allProjects: [ProjectState] {
        projectNodes.flatMap { node -> [ProjectState] in
            switch node.kind {
            case .project(let p): return [p]
            case .machine: return node.children.compactMap { if case .project(let p) = $0.kind { p } else { nil } }
            default: return []
            }
        }
    }

    private func findProject(byRoot projectId: String) -> ProjectState? {
        allProjects.first { $0.projectId == projectId }
    }

    private func findProject(forWorktreeId wtId: String) -> ProjectState? {
        allProjects.first { $0.worktrees.contains(where: { $0.id == wtId }) }
    }

    private func findProject(forAgentId agentId: String) -> ProjectState? {
        allProjects.first { $0.agent(byId: agentId) != nil }
    }

    private func findProject(forWorkspaceId workspaceId: String) -> ProjectState? {
        allProjects.first { workspacesByProject[$0.projectId]?.contains(where: { $0.id == workspaceId }) == true }
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

        // A menu opened from the keyboard (or VoiceOver) has no clicked row: use the selection.
        let clickedRow = outlineView.clickedRow >= 0 ? outlineView.clickedRow : outlineView.selectedRow
        guard clickedRow >= 0, let node = outlineView.item(atRow: clickedRow) as? SidebarNode else { return }
        contextClickedNode = node

        switch node.kind {
        case .workspace(let workspace): buildWorkspaceContextMenu(menu, workspace: workspace)
        case .worktree(let worktree): buildWorktreeContextMenu(menu, worktree: worktree)
        case .project(let project): buildProjectContextMenu(menu, project: project)
        case .machine(let machine): buildMachineContextMenu(menu, machine: machine)
        case .sectionHeader: break
        }
    }

    private func buildMachineContextMenu(_ menu: NSMenu, machine: MachineState) {
        if machine.isConnectedOrConnecting {
            menu.addItem(makeMenuItem(title: "Disconnect", action: #selector(contextDisconnectMachine(_:))))
        } else {
            menu.addItem(makeMenuItem(title: "Connect", action: #selector(contextConnectMachine(_:))))
        }
        if machine.state == .connected(.stopped) {
            let title = machine.isStartingDaemon ? "Starting Daemon\u{2026}" : "Start Daemon"
            let item = makeMenuItem(title: title, action: #selector(contextStartDaemon(_:)))
            item.isEnabled = !machine.isStartingDaemon
            menu.addItem(item)
        }
        if machine.hiddenProjectCount > 0 {
            menu.addItem(
                makeMenuItem(
                    title: "Show Hidden Projects (\(machine.hiddenProjectCount))",
                    action: #selector(contextShowHiddenProjects(_:))))
        }
        if machine.actionError != nil {
            menu.addItem(makeMenuItem(title: "Dismiss Error", action: #selector(contextDismissMachineError(_:))))
        }
        menu.addItem(.separator())
        menu.addItem(makeMenuItem(title: "Remove Machine\u{2026}", action: #selector(contextRemoveMachine(_:))))
    }

    @objc private func contextShowHiddenProjects(_ sender: NSMenuItem) {
        contextMachine?.showHiddenProjects()
    }

    @objc private func contextDismissMachineError(_ sender: NSMenuItem) {
        contextMachine?.actionError = nil
    }

    @objc private func contextRetryLayout(_ sender: NSMenuItem) {
        guard let node = contextClickedNode, case .project(let project) = node.kind else { return }
        project.retryRemoteLayout()
    }

    /// Return on a selected machine row connects or disconnects it.
    func toggleSelectedMachineConnection() -> Bool {
        let row = outlineView.selectedRow
        guard row >= 0, let node = outlineView.item(atRow: row) as? SidebarNode,
            case .machine(let machine) = node.kind
        else { return false }
        if machine.isConnectedOrConnecting { confirmDisconnect(machine) } else { machine.connect() }
        return true
    }

    private var contextMachine: MachineState? {
        guard let node = contextClickedNode, case .machine(let machine) = node.kind else { return nil }
        return machine
    }

    @objc private func contextConnectMachine(_ sender: NSMenuItem) {
        contextMachine?.connect()
    }

    @objc private func contextDisconnectMachine(_ sender: NSMenuItem) {
        guard let machine = contextMachine else { return }
        confirmDisconnect(machine)
    }

    private func confirmDisconnect(_ machine: MachineState) {
        let unsaved = machine.unsavedFileNames
        guard !unsaved.isEmpty else {
            machine.disconnect(remember: true)
            return
        }
        showConfirmation(
            title: "Disconnect \(machine.machine.name)?",
            message: "Its projects close in PurePoint until you connect again." + Self.unsavedWarning(unsaved),
            confirmTitle: "Disconnect"
        ) {
            machine.disconnect(remember: true)
        }
    }

    /// Closing a remote project drops its file tabs, edits included.
    private static func unsavedWarning(_ names: [String]) -> String {
        names.isEmpty ? "" : " Unsaved changes to \(names.joined(separator: ", ")) will be lost."
    }

    @objc private func contextStartDaemon(_ sender: NSMenuItem) {
        contextMachine?.startDaemon()
    }

    @objc private func contextRemoveMachine(_ sender: NSMenuItem) {
        guard let machine = contextMachine else { return }
        confirmRemove(machine)
    }

    private func confirmRemove(_ machine: MachineState) {
        showConfirmation(
            title: "Remove \(machine.machine.name)?",
            message:
                "PurePoint will close its connection and forget it. The daemon, projects and agents on "
                + "\(machine.machine.name) keep running and are not changed."
                + Self.unsavedWarning(machine.unsavedFileNames),
            confirmTitle: "Remove"
        ) {
            self.onRemoveMachine?(machine)
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
        if project.isRemote, project.registry?.layoutErrorsByProject[project.projectId] != nil {
            menu.addItem(makeMenuItem(title: "Retry Layout", action: #selector(contextRetryLayout(_:))))
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
        let unsaved =
            project.isRemote
            ? project.appState?.machines.first { $0.machine.id == project.key.machineId }?.unsavedFileNames(in: project)
                ?? []
            : []
        showConfirmation(
            title: "Remove \(project.projectName)?",
            message: project.isRemote
                ? "This hides the project in PurePoint on this Mac. Nothing on the remote machine is changed."
                    + Self.unsavedWarning(unsaved)
                : "This will close the project in PurePoint and delete .pu/manifest.json and .pu/agents/. "
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

/// The sidebar outline, with Return acting on a selected machine row.
final class SidebarKeyOutlineView: NSOutlineView {
    /// Return pressed; true if handled.
    var onReturn: (() -> Bool)?
    /// Called once this view has joined a window, so a focus request made before that can land.
    var onWindowAttached: ((NSOutlineView) -> Void)?

    override func keyDown(with event: NSEvent) {
        if event.keyCode == 36 || event.keyCode == 76, onReturn?() == true { return }
        super.keyDown(with: event)
    }

    override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        guard window != nil else { return }
        onWindowAttached?(self)
    }
}
#endif
