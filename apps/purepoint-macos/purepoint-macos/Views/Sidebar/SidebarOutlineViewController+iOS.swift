#if os(iOS)
import UIKit

/// iOS twin of the Mac sidebar outline: a hierarchical `UICollectionView` fed by a diffable
/// section snapshot, with the Mac's rows, metrics, selection, expansion, context menus, inline
/// rename and keyboard behaviour. The tree, menus and actions come from the same shared model
/// as the Mac (`SidebarTree`, `SidebarMenus`, `perform(_:on:)`); only the drawing is UIKit.
///
/// The Mac sidebar offers no drags (its outline has no pasteboard writer), so neither does this.
@MainActor
class SidebarOutlineViewController: UIViewController {

    let outlineView: SidebarKeyOutlineView

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

    /// Which rows the user closed. Every rebuild derives the snapshot's expansion from this,
    /// so a refresh never reopens a closed row or closes an open one.
    private let expansionStore: SidebarExpansionStore

    var dataSource: UICollectionViewDiffableDataSource<Int, String>!
    /// The row behind each item identifier (its `rowKey`) in the current tree.
    var nodesByKey: [String: SidebarNode] = [:]

    /// Inline rename state.
    var editingTextField: SidebarRenameField?
    var editingOriginalName: String?
    var editingWorkspaceId: String?
    /// A rename chosen from a context menu, started once the menu has gone.
    var pendingRenameKey: String?
    /// The latest input that arrived mid-rename, applied when the rename ends.
    var deferredRebuild: (projects: [ProjectState], machines: [MachineState])?

    init(expansionStore: SidebarExpansionStore) {
        self.expansionStore = expansionStore
        outlineView = SidebarKeyOutlineView(frame: .zero, collectionViewLayout: Self.makeLayout())
        super.init(nibName: nil, bundle: nil)
        configureOutlineView()
    }

    required init?(coder: NSCoder) {
        fatalError("init(coder:) is not supported: the sidebar is built in code")
    }

    // MARK: - Lifecycle

    override func loadView() {
        view = outlineView
    }

    /// Fixed 24pt rows, as the Mac outline's `heightOfRowByItem`.
    private static func makeLayout() -> UICollectionViewLayout {
        let size = NSCollectionLayoutSize(
            widthDimension: .fractionalWidth(1), heightDimension: .absolute(PurePointTheme.sidebarRowHeight))
        let group = NSCollectionLayoutGroup.vertical(layoutSize: size, subitems: [NSCollectionLayoutItem(layoutSize: size)])
        return UICollectionViewCompositionalLayout(section: NSCollectionLayoutSection(group: group))
    }

    private func configureOutlineView() {
        outlineView.backgroundColor = .clear
        outlineView.delegate = self
        outlineView.allowsMultipleSelection = false
        // The outline takes arrow keys itself, as the Mac outline does; the focus system would
        // otherwise move a focus ring instead of the selection.
        outlineView.allowsFocus = false
        outlineView.dragInteractionEnabled = false
        outlineView.alwaysBounceVertical = true
        outlineView.onReturn = { [weak self] in self?.toggleSelectedMachineConnection() ?? false }
        outlineView.onArrowKey = { [weak self] key in self?.handleArrowKey(key) ?? false }
        outlineView.onFirstResponderChanged = { [weak self] in self?.updateEmphasis() }

        let registration = UICollectionView.CellRegistration<SidebarRowCell, String> { [weak self] cell, _, key in
            self?.configure(cell, key: key)
        }
        dataSource = UICollectionViewDiffableDataSource(collectionView: outlineView) { collectionView, indexPath, key in
            collectionView.dequeueConfiguredReusableCell(using: registration, for: indexPath, item: key)
        }

        // A click below the rows clears the selection, as it does in the Mac outline.
        let backgroundTap = UITapGestureRecognizer(target: self, action: #selector(backgroundTapped(_:)))
        backgroundTap.cancelsTouchesInView = false
        backgroundTap.delegate = self
        outlineView.addGestureRecognizer(backgroundTap)
    }

    // MARK: - Data Rebuild

    /// Rebuild the node tree from AppState projects, then a MACHINES section with each
    /// machine's projects nested under it.
    func rebuildNodes(projects: [ProjectState], machines: [MachineState] = []) {
        // Reconfiguring mid-rename would rebuild the row holding the field. The input is kept
        // and applied once the rename ends.
        guard editingTextField == nil else {
            deferredRebuild = (projects, machines)
            return
        }
        let nextRenderState = SidebarRenderState.make(
            projects: projects, machines: machines, workspacesByProject: workspacesByProject,
            activeWorkspaceId: activeWorkspaceId)
        guard nextRenderState != lastRenderState else { return }

        let oldSelectedId = selectedNodeId()
        let scrollOffset = outlineView.contentOffset

        projectNodes = SidebarTree.build(projects: projects, machines: machines, workspacesByProject: workspacesByProject)
        lastRenderState = nextRenderState
        applyTree(animated: false)
        restoreExpansionState()
        reconfigureVisibleRows()

        if let oldSelectedId {
            selectNode(withId: oldSelectedId)
        }

        outlineView.layoutIfNeeded()
        let maxOffsetY = max(
            -outlineView.adjustedContentInset.top,
            outlineView.contentSize.height - outlineView.bounds.height + outlineView.adjustedContentInset.bottom)
        outlineView.contentOffset = CGPoint(x: scrollOffset.x, y: min(scrollOffset.y, maxOffsetY))
    }

    /// Feeds the tree to the collection view. Every expandable row the user has not closed is
    /// expanded — including rows inside a closed row, so they come back open with it.
    private func applyTree(animated: Bool) {
        var section = NSDiffableDataSourceSectionSnapshot<String>()
        var nodes: [String: SidebarNode] = [:]
        func append(_ children: [SidebarNode], under parent: String?) {
            // Item identifiers must be unique; a row key repeated in the input is shown once.
            let fresh = children.filter { nodes[$0.rowKey] == nil }
            for node in fresh { nodes[node.rowKey] = node }
            section.append(fresh.map(\.rowKey), to: parent)
            for node in fresh { append(node.children, under: node.rowKey) }
        }
        append(projectNodes, under: nil)
        nodesByKey = nodes

        let closed = expansionStore.closedRowKeys
        section.expand(nodes.values.filter { $0.isExpandable && !closed.contains($0.rowKey) }.map(\.rowKey))
        dataSource.apply(section, to: 0, animatingDifferences: animated)
    }

    /// Drops recorded rows that no longer exist — but only once every row has loaded, since an
    /// empty row before then has simply not answered yet. See `SidebarTree.isSettled(_:)`.
    private func restoreExpansionState() {
        let expansion = SidebarTree.expansion(projectNodes, closedRowKeys: expansionStore.closedRowKeys)
        guard SidebarTree.isSettled(projectNodes) else { return }
        expansionStore.record(expansion.closed)
    }

    /// Reapplies row content to the cells on screen; the rest are configured when they scroll in.
    private func reconfigureVisibleRows(_ keys: Set<String>? = nil) {
        for indexPath in outlineView.indexPathsForVisibleItems {
            guard let key = dataSource.itemIdentifier(for: indexPath), keys?.contains(key) ?? true,
                let cell = outlineView.cellForItem(at: indexPath) as? SidebarRowCell
            else { continue }
            configure(cell, key: key)
        }
    }

    private var section: NSDiffableDataSourceSectionSnapshot<String> {
        dataSource.snapshot(for: 0)
    }

    func node(at indexPath: IndexPath) -> SidebarNode? {
        dataSource.itemIdentifier(for: indexPath).flatMap { nodesByKey[$0] }
    }

    private var selectedIndexPath: IndexPath? {
        outlineView.indexPathsForSelectedItems?.first
    }

    private func selectedNodeId() -> String? {
        selectedIndexPath.flatMap(node(at:))?.id
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
        for indexPath in outlineView.indexPathsForSelectedItems ?? [] {
            outlineView.deselectItem(at: indexPath, animated: false)
        }
        suppressSelectionCallback = false
    }

    private func selectNode(withId targetId: String) {
        if selectedNodeId() == targetId {
            return
        }

        // A row inside a closed row has no row of its own yet, so its ancestors are opened
        // first — otherwise selecting a workspace inside a collapsed project would silently
        // find nothing.
        let closedAncestors = SidebarTree.ancestors(where: { $0.id == targetId }, in: projectNodes)
            .filter { expansionStore.closedRowKeys.contains($0.rowKey) || !section.isExpanded($0.rowKey) }
        for ancestor in closedAncestors {
            setExpanded(ancestor.rowKey, true, animated: false)
        }

        guard
            let key = section.visibleItems.first(where: { nodesByKey[$0]?.id == targetId }),
            let indexPath = dataSource.indexPath(for: key)
        else { return }
        suppressSelectionCallback = true
        outlineView.selectItem(at: indexPath, animated: false, scrollPosition: [])
        suppressSelectionCallback = false
    }

    /// A user selection (tap or arrow key): what the Mac's `outlineViewSelectionDidChange` does.
    private func userSelected(_ indexPath: IndexPath?) {
        NSLog("PUDBG userSelected %@ suppress=%d", "\(String(describing: indexPath))", suppressSelectionCallback ? 1 : 0)
        guard !suppressSelectionCallback else { return }
        guard let indexPath, let node = node(at: indexPath) else {
            onSelectionChanged?(nil)
            return
        }
        guard let selection = SidebarTree.selection(for: node) else { return }
        onSelectionChanged?(selection)
    }

    // MARK: - Row Expansion

    /// Opens or closes a row and records it, as the Mac's expand and collapse notifications do.
    /// Opening a row brings back every row under it the user did not close.
    func setExpanded(_ key: String, _ isExpanded: Bool, animated: Bool = true) {
        guard let node = nodesByKey[key], node.isExpandable else { return }
        let wasSelected = selectedIndexPath
        expansionStore.setExpanded(key, isExpanded: isExpanded)

        var section = self.section
        if isExpanded {
            let closed = expansionStore.closedRowKeys
            func reopen(_ children: [SidebarNode]) -> [String] {
                children.filter { $0.isExpandable && !closed.contains($0.rowKey) }
                    .flatMap { [$0.rowKey] + reopen($0.children) }
            }
            section.expand([key] + reopen(node.children))
        } else {
            section.collapse([key])
        }
        dataSource.apply(section, to: 0, animatingDifferences: animated)
        reconfigureVisibleRows([key])

        // A selected row that closed out of sight is no longer selected, as in the Mac outline.
        if wasSelected != nil, selectedIndexPath == nil {
            userSelected(nil)
        }
    }

    func isExpanded(_ key: String) -> Bool {
        section.isExpanded(key)
    }

    private func toggleExpansion(_ key: String) {
        setExpanded(key, !section.isExpanded(key))
    }

    // MARK: - Keyboard

    /// The visible rows as the key navigation sees them.
    private func navigationRows() -> (keys: [String], rows: [SidebarKeyNavigation.Row]) {
        let section = self.section
        let keys = section.visibleItems
        let index = Dictionary(uniqueKeysWithValues: keys.enumerated().map { ($1, $0) })
        let rows = keys.map { key in
            let node = nodesByKey[key]
            return SidebarKeyNavigation.Row(
                parent: section.parent(of: key).flatMap { index[$0] },
                isExpandable: node?.isExpandable ?? false,
                isExpanded: section.isExpanded(key),
                isSelectable: node.map(SidebarTree.isSelectable) ?? false)
        }
        return (keys, rows)
    }

    func handleArrowKey(_ key: SidebarKeyNavigation.Key) -> Bool {
        guard editingTextField == nil else { return false }
        let (keys, rows) = navigationRows()
        let selected = selectedIndexPath.flatMap { dataSource.itemIdentifier(for: $0) }.flatMap { keys.firstIndex(of: $0) }
        switch SidebarKeyNavigation.command(for: key, rows: rows, selected: selected) {
        case .select(let row):
            guard let indexPath = dataSource.indexPath(for: keys[row]) else { return true }
            outlineView.selectItem(at: indexPath, animated: false, scrollPosition: [])
            scrollRowToVisible(indexPath)
            userSelected(indexPath)
        case .expand(let row):
            setExpanded(keys[row], true)
        case .collapse(let row):
            setExpanded(keys[row], false)
        case .none:
            break
        }
        return true
    }

    /// Scrolls just enough to show the row, as `scrollRowToVisible` does.
    private func scrollRowToVisible(_ indexPath: IndexPath) {
        guard let frame = outlineView.layoutAttributesForItem(at: indexPath)?.frame else { return }
        outlineView.scrollRectToVisible(frame, animated: false)
    }

    /// Return on a selected machine row connects or disconnects it.
    func toggleSelectedMachineConnection() -> Bool {
        guard let indexPath = selectedIndexPath, let node = node(at: indexPath),
            case .machine(let machine) = node.kind
        else { return false }
        toggleConnection(of: machine)
        return true
    }

    private func updateEmphasis() {
        let emphasized = outlineView.isFirstResponder
        for case let cell as SidebarRowCell in outlineView.visibleCells {
            cell.isEmphasized = emphasized
        }
    }

    @objc private func backgroundTapped(_ recognizer: UITapGestureRecognizer) {
        NSLog("PUDBG backgroundTapped")
        outlineView.becomeFirstResponder()
        guard selectedIndexPath != nil else { return }
        deselectAll()
        userSelected(nil)
    }

    // MARK: - Cells

    private func configure(_ cell: SidebarRowCell, key: String) {
        guard let node = nodesByKey[key] else { return }
        let section = self.section
        cell.isEmphasized = outlineView.isFirstResponder
        cell.onToggleExpansion = { [weak self] in self?.toggleExpansion(key) }
        cell.fillContent = { [weak self] cell in self?.fillContent(cell, node: node) }
        cell.configure(
            level: section.contains(key) ? section.level(of: key) : 0, isExpandable: node.isExpandable,
            isExpanded: section.contains(key) && section.isExpanded(key))
    }

    private func fillContent(_ cell: SidebarRowCell, node: SidebarNode) {
        let content = SidebarRowContent(emphasized: cell.showsEmphasizedSelection)
        cell.toolTip = nil
        cell.accessibilityLabel = nil
        cell.accessibilityCustomActions = nil
        switch node.kind {
        case .project(let project): fillProjectRow(cell, project: project, content: content)
        case .worktree(let worktree): fillWorktreeRow(cell, worktree: worktree, node: node, content: content)
        case .workspace(let workspace): fillWorkspaceRow(cell, workspace: workspace, content: content)
        case .sectionHeader(let title): fillSectionHeaderRow(cell, title: title, content: content)
        case .machine(let machine): fillMachineRow(cell, machine: machine, node: node, content: content)
        }
    }

    private func fillProjectRow(_ cell: SidebarRowCell, project: ProjectState, content: SidebarRowContent) {
        let stack = cell.resetStack(spacing: 6)

        let icon = content.symbolIcon("folder.fill", description: "Project", pointSize: 11)
        let name = content.label(project.projectName, font: .systemFont(ofSize: 12, weight: .semibold))

        let projectId = project.projectId
        let addBtn = content.inlineAddButton(
            action: UIAction { [weak self] _ in
                guard let self, let project = self.findProject(byRoot: projectId) else { return }
                self.onShowCommandPalette?(project, nil, true)
            })

        stack.addArrangedSubview(icon)
        stack.addArrangedSubview(name)
        stack.addArrangedSubview(content.spacer())
        if project.isUnavailable {
            // Kept only for its unsaved file tabs while the machine's daemon is unreachable.
            name.textColor = content.tone(.secondaryLabelColor)
            let status = content.label("Unavailable", font: .systemFont(ofSize: 10), color: .secondaryLabelColor)
            stack.addArrangedSubview(status)
            cell.toolTip = "The machine's daemon is unreachable. Unsaved file tabs are kept until it is back."
            cell.accessibilityLabel = "\(project.projectName), unavailable"
        } else if project.isRemote, let error = project.registry?.layoutErrorsByProject[project.projectId] {
            let warning = content.symbolIcon("exclamationmark.triangle.fill", description: "Layout error", pointSize: 10)
            warning.tintColor = .systemOrange
            stack.addArrangedSubview(warning)
            cell.toolTip = error + " Retrying; choose Retry Layout to try now."
            cell.accessibilityLabel = "\(project.projectName), \(error)"
        }
        if project.channel.unreadCount > 0 {
            let badge = content.badge(count: project.channel.unreadCount)
            badge.accessibilityIdentifier = "channelUnreadCount"
            badge.textColor = content.tone(.controlAccentColor)
            badge.accessibilityLabel = "\(project.channel.unreadCount) unread channel messages"
            stack.addArrangedSubview(badge)
        }
        stack.addArrangedSubview(addBtn)
    }

    private func fillSectionHeaderRow(_ cell: SidebarRowCell, title: String, content: SidebarRowContent) {
        let stack = cell.resetStack(spacing: 6)
        let label = content.label(title, font: .systemFont(ofSize: 10, weight: .semibold), color: .secondaryLabelColor)
        stack.addArrangedSubview(label)
        stack.addArrangedSubview(content.spacer())
        cell.accessibilityLabel = "Machines"
    }

    private func fillMachineRow(
        _ cell: SidebarRowCell, machine: MachineState, node: SidebarNode, content: SidebarRowContent
    ) {
        let stack = cell.resetStack(spacing: 6)

        stack.addArrangedSubview(content.statusDot(color: machine.state.sidebarStatusColor))
        let icon = content.symbolIcon("desktopcomputer", description: "Machine", pointSize: 11)
        let name = content.label(machine.machine.name, font: .systemFont(ofSize: 12, weight: .semibold))
        let status = content.label(machine.statusText, font: .systemFont(ofSize: 10), color: .secondaryLabelColor)
        status.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)

        stack.addArrangedSubview(icon)
        stack.addArrangedSubview(name)
        stack.addArrangedSubview(content.spacer())
        stack.addArrangedSubview(status)
        var tooltip = "\(machine.machine.name) (\(machine.machine.sshHost)): \(machine.statusText)"
        if case .offline(let reason) = machine.state { tooltip += " — " + reason }
        if let error = machine.actionError {
            let warning = content.symbolIcon("exclamationmark.triangle.fill", description: "Error", pointSize: 10)
            warning.tintColor = .systemOrange
            stack.addArrangedSubview(warning)
            tooltip += "\n" + error
        }
        cell.toolTip = tooltip
        cell.accessibilityLabel = tooltip
        cell.accessibilityCustomActions = machineAccessibilityActions(machine, key: node.rowKey)
    }

    private func fillWorktreeRow(
        _ cell: SidebarRowCell, worktree: WorktreeModel, node: SidebarNode, content: SidebarRowContent
    ) {
        let stack = cell.resetStack(spacing: 6)

        let icon = content.symbolIcon("arrow.triangle.branch", description: "Worktree", pointSize: 10)
        let name = content.label(worktree.branch, font: .systemFont(ofSize: 12))

        let addBtn = content.inlineAddButton(menu: makeMenu(SidebarMenus.worktreeAddMenu, key: node.rowKey))

        stack.addArrangedSubview(icon)
        stack.addArrangedSubview(name)
        stack.addArrangedSubview(content.spacer())
        stack.addArrangedSubview(addBtn)

        if worktree.agents.count > 0 {
            stack.addArrangedSubview(content.badge(count: worktree.agents.count))
        }
    }

    /// One row per workspace. A multi-pane workspace shows a pane count instead of
    /// expanding into child rows — its panes are only ever reachable inside its grid.
    private func fillWorkspaceRow(_ cell: SidebarRowCell, workspace: Workspace, content: SidebarRowContent) {
        let stack = cell.resetStack(spacing: 5)

        guard let project = findProject(forWorkspaceId: workspace.id) else { return }
        let state = SidebarRenderState.workspace(workspace, in: project)

        stack.addArrangedSubview(content.statusDot(color: state.status?.nsColor ?? .tertiaryLabelColor))

        let label = content.label(state.title, font: .systemFont(ofSize: 11))
        label.setContentHuggingPriority(.defaultHigh, for: .horizontal)
        label.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        stack.addArrangedSubview(label)
        cell.nameLabel = label
        if let signal = state.signal {
            stack.addArrangedSubview(content.pill(signal))
        }

        stack.addArrangedSubview(content.spacer())
        if state.paneCount > 1 {
            stack.addArrangedSubview(content.countIcon("rectangle.split.2x2", description: "Panes", count: state.paneCount))
        }
        if state.tabCount > state.paneCount {
            stack.addArrangedSubview(content.countIcon("square.stack", description: "Tabs", count: state.tabCount))
        }

        let paneNote = state.paneCount > 1 ? ", \(state.paneCount) panes" : ""
        let tabNote = state.tabCount > state.paneCount ? ", \(state.tabCount) tabs" : ""
        cell.accessibilityLabel =
            "\(state.title), \(state.signal?.accessibilityDescription ?? state.status?.rawValue ?? "files")\(paneNote)\(tabNote)"
    }
}

/// Set while a sidebar row is being renamed so terminals don't take first responder
/// away from the field editor when the selection switches content.
enum InlineRenameFocus {
    static var isActive = false
}

// MARK: - UICollectionViewDelegate

extension SidebarOutlineViewController: UICollectionViewDelegate {
    func collectionView(_ collectionView: UICollectionView, shouldSelectItemAt indexPath: IndexPath) -> Bool {
        NSLog("PUDBG shouldSelect %@ %@", "\(indexPath)", node(at: indexPath)?.rowKey ?? "nil")
        guard let node = node(at: indexPath), SidebarTree.isSelectable(node) else { return false }
        // Clicking the selected row again changes nothing, as in the Mac outline.
        if collectionView.indexPathsForSelectedItems?.contains(indexPath) == true {
            outlineView.becomeFirstResponder()
            return false
        }
        return true
    }

    func collectionView(_ collectionView: UICollectionView, shouldDeselectItemAt indexPath: IndexPath) -> Bool {
        // Single selection: only a new selection or a click below the rows clears a row.
        false
    }

    func collectionView(_ collectionView: UICollectionView, didSelectItemAt indexPath: IndexPath) {
        NSLog("PUDBG didSelect %@", "\(indexPath)")
        if editingTextField == nil {
            outlineView.becomeFirstResponder()
        }
        userSelected(indexPath)
    }

    func collectionView(_ collectionView: UICollectionView, willDisplay cell: UICollectionViewCell, forItemAt indexPath: IndexPath) {
        (cell as? SidebarRowCell)?.isEmphasized = outlineView.isFirstResponder
    }

}

// MARK: - Background clicks

extension SidebarOutlineViewController: UIGestureRecognizerDelegate {
    func gestureRecognizerShouldBegin(_ gestureRecognizer: UIGestureRecognizer) -> Bool {
        outlineView.indexPathForItem(at: gestureRecognizer.location(in: outlineView)) == nil
    }

    func gestureRecognizer(
        _ gestureRecognizer: UIGestureRecognizer, shouldRecognizeSimultaneouslyWith other: UIGestureRecognizer
    ) -> Bool {
        true
    }
}

#endif
