#if os(iOS)
import UIKit

/// iOS twin of the Mac file-tree outline: a hierarchical `UICollectionView` fed by a diffable
/// section snapshot, with the Mac's 24pt rows, 12pt indentation, icons, lazy folder loading,
/// selection, context menu and keyboard behaviour. Only the drawing is UIKit; the tree comes
/// from the same `FileTreeState`, which lists remote trees through the machine's daemon.
///
/// The Mac tree offers no drags (its outline has no pasteboard writer), so neither does this.
@MainActor
class FileTreeOutlineViewController: UIViewController {

    let outlineView: FileTreeKeyOutlineView

    var rootNodes: [FileTreeNode] = []

    var onFileSelected: ((String, String) -> Void)?  // (absolutePath, name)
    var onNodeExpanded: ((FileTreeNode) -> Void)?
    var onNodeCollapsed: ((FileTreeNode) -> Void)?
    /// Remote trees cannot be revealed in Finder; their files open as fetched local copies.
    var isLocal = true
    var onOpenLocalCopy: ((String) -> Void)?

    /// Rows are keyed by absolute path, which is unique within one tree.
    private(set) var dataSource: UICollectionViewDiffableDataSource<Int, String>!
    private var nodesByPath: [String: FileTreeNode] = [:]
    /// Folders the user opened. Survives reloads, as the Mac controller's set does.
    private(set) var expandedPaths: Set<String> = []
    private var selectedPath: String?
    private var typeSelect = FileTreeTypeSelect()

    init() {
        outlineView = FileTreeKeyOutlineView(frame: .zero, collectionViewLayout: Self.makeLayout())
        super.init(nibName: nil, bundle: nil)
        configureOutlineView()
    }

    required init?(coder: NSCoder) {
        fatalError("init(coder:) is not supported: the file tree is built in code")
    }

    // MARK: - Lifecycle

    override func loadView() {
        view = outlineView
    }

    /// Fixed 24pt rows, as the Mac outline's `heightOfRowByItem`.
    private static func makeLayout() -> UICollectionViewLayout {
        let size = NSCollectionLayoutSize(widthDimension: .fractionalWidth(1), heightDimension: .absolute(24))
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
        outlineView.onArrowKey = { [weak self] key in self?.handleArrowKey(key) ?? false }
        outlineView.onTypeSelect = { [weak self] characters in self?.handleTypeSelect(characters) ?? false }
        outlineView.onFirstResponderChanged = { [weak self] in self?.updateEmphasis() }

        let registration = UICollectionView.CellRegistration<FileTreeRowCell, String> { [weak self] cell, _, path in
            self?.configure(cell, path: path)
        }
        dataSource = UICollectionViewDiffableDataSource(collectionView: outlineView) { collectionView, indexPath, path in
            collectionView.dequeueConfiguredReusableCell(using: registration, for: indexPath, item: path)
        }

        // A click below the rows clears the selection, as it does in the Mac outline.
        let backgroundTap = UITapGestureRecognizer(target: self, action: #selector(backgroundTapped(_:)))
        backgroundTap.cancelsTouchesInView = false
        backgroundTap.delegate = self
        outlineView.addGestureRecognizer(backgroundTap)
    }

    // MARK: - Data Update

    func updateNodes(_ nodes: [FileTreeNode]) {
        if nodes.count == rootNodes.count && zip(nodes, rootNodes).allSatisfy({ $0 === $1 }) {
            return
        }
        rootNodes = nodes
        applyTree()
    }

    /// Feeds the tree to the collection view, opening every folder in `expandedPaths`. The
    /// diffable apply keeps the scroll position, as the Mac restores its clip view origin.
    private func applyTree() {
        var section = NSDiffableDataSourceSectionSnapshot<String>()
        var nodes: [String: FileTreeNode] = [:]
        func append(_ children: [FileTreeNode], under parent: String?) {
            // Item identifiers must be unique; a path repeated in the input is shown once.
            let fresh = children.filter { nodes[$0.absolutePath] == nil }
            for node in fresh { nodes[node.absolutePath] = node }
            section.append(fresh.map(\.absolutePath), to: parent)
            for node in fresh { append(node.children, under: node.absolutePath) }
        }
        append(rootNodes, under: nil)
        nodesByPath = nodes
        section.expand(expandedPaths.filter { nodes[$0]?.isDirectory == true })
        dataSource.apply(section, to: 0, animatingDifferences: false)
        reconfigureVisibleRows()
        restoreSelection()
    }

    /// The selection follows its file across reloads without reopening it.
    private func restoreSelection() {
        guard let selectedPath else { return }
        guard let indexPath = dataSource.indexPath(for: selectedPath) else {
            self.selectedPath = nil
            return
        }
        if outlineView.indexPathsForSelectedItems?.contains(indexPath) != true {
            outlineView.selectItem(at: indexPath, animated: false, scrollPosition: [])
        }
    }

    /// Reapplies row content to the cells on screen; the rest are configured when they scroll in.
    private func reconfigureVisibleRows() {
        for indexPath in outlineView.indexPathsForVisibleItems {
            guard let path = dataSource.itemIdentifier(for: indexPath),
                let cell = outlineView.cellForItem(at: indexPath) as? FileTreeRowCell
            else { continue }
            configure(cell, path: path)
        }
    }

    private var section: NSDiffableDataSourceSectionSnapshot<String> {
        dataSource.snapshot(for: 0)
    }

    func node(at indexPath: IndexPath) -> FileTreeNode? {
        dataSource.itemIdentifier(for: indexPath).flatMap { nodesByPath[$0] }
    }

    /// The rows on screen top to bottom, as the Mac outline's rows.
    var visibleNodes: [FileTreeNode] {
        section.visibleItems.compactMap { nodesByPath[$0] }
    }

    func isExpanded(_ node: FileTreeNode) -> Bool {
        expandedPaths.contains(node.absolutePath)
    }

    // MARK: - Expansion

    func toggleExpansion(_ node: FileTreeNode) {
        setExpanded(node, !isExpanded(node))
    }

    /// Opens or closes a folder. Opening an unloaded folder asks the state for its children
    /// (`outlineViewItemDidExpand`); closing tells the state to drop them (`outlineViewItemWillCollapse`).
    func setExpanded(_ node: FileTreeNode, _ expanded: Bool) {
        guard node.isExpandable, isExpanded(node) != expanded else { return }
        let path = node.absolutePath
        if expanded {
            expandedPaths.insert(path)
            var section = self.section
            if section.contains(path) { section.expand([path]) }
            dataSource.apply(section, to: 0, animatingDifferences: false)
            reconfigureRow(path)
            guard node.children.isEmpty else { return }  // already loaded
            onNodeExpanded?(node)
            // A local folder loads in place; show its children as the Mac reloads the item.
            DispatchQueue.main.async { [weak self] in
                guard let self, !node.children.isEmpty else { return }
                self.applyTree()
            }
        } else {
            expandedPaths.remove(path)
            onNodeCollapsed?(node)
            var section = self.section
            if section.contains(path) { section.collapse([path]) }
            dataSource.apply(section, to: 0, animatingDifferences: false)
            reconfigureRow(path)
            // A selection inside the closed folder goes, as in the Mac outline.
            if let selectedPath, selectedPath.hasPrefix(path + "/") {
                self.selectedPath = nil
            }
        }
    }

    private func reconfigureRow(_ path: String) {
        guard let indexPath = dataSource.indexPath(for: path),
            let cell = outlineView.cellForItem(at: indexPath) as? FileTreeRowCell
        else { return }
        configure(cell, path: path)
    }

    // MARK: - Selection

    /// The Mac's `outlineViewSelectionDidChange`: a file opens, a folder only selects.
    private func userSelected(_ indexPath: IndexPath?) {
        guard let indexPath, let node = node(at: indexPath) else {
            selectedPath = nil
            return
        }
        guard selectedPath != node.absolutePath else { return }
        selectedPath = node.absolutePath
        if !node.isDirectory {
            onFileSelected?(node.absolutePath, node.name)
        }
    }

    /// Selects and reveals a row as a keyboard selection does.
    private func select(_ node: FileTreeNode) {
        guard let indexPath = dataSource.indexPath(for: node.absolutePath) else { return }
        outlineView.selectItem(at: indexPath, animated: false, scrollPosition: [])
        outlineView.scrollToItem(at: indexPath, at: [], animated: false)
        userSelected(indexPath)
    }

    private func deselectAll() {
        for indexPath in outlineView.indexPathsForSelectedItems ?? [] {
            outlineView.deselectItem(at: indexPath, animated: false)
        }
        userSelected(nil)
    }

    private func updateEmphasis() {
        let emphasized = outlineView.isFirstResponder
        for case let cell as FileTreeRowCell in outlineView.visibleCells {
            cell.isEmphasized = emphasized
        }
    }

    @objc private func backgroundTapped(_ recognizer: UITapGestureRecognizer) {
        outlineView.becomeFirstResponder()
        deselectAll()
    }

    // MARK: - Keyboard

    func handleArrowKey(_ key: FileTreeKeyNavigation.Key) -> Bool {
        let section = self.section
        let visible = section.visibleItems
        var indexByPath: [String: Int] = [:]
        for (index, path) in visible.enumerated() { indexByPath[path] = index }
        let rows = visible.map { path -> FileTreeKeyNavigation.Row in
            let node = nodesByPath[path]
            return FileTreeKeyNavigation.Row(
                parent: section.parent(of: path).flatMap { indexByPath[$0] },
                isExpandable: node?.isExpandable ?? false,
                isExpanded: node.map(isExpanded) ?? false)
        }
        let selected = selectedPath.flatMap { indexByPath[$0] }
        switch FileTreeKeyNavigation.command(for: key, rows: rows, selected: selected) {
        case .select(let index):
            if let node = nodesByPath[visible[index]] { select(node) }
        case .expand(let index):
            if let node = nodesByPath[visible[index]] { setExpanded(node, true) }
        case .collapse(let index):
            if let node = nodesByPath[visible[index]] { setExpanded(node, false) }
        case .none:
            return false
        }
        return true
    }

    func handleTypeSelect(_ characters: String) -> Bool {
        let nodes = visibleNodes
        let selected = selectedPath.flatMap { path in nodes.firstIndex { $0.absolutePath == path } }
        guard let index = typeSelect.row(typing: characters, names: nodes.map(\.name), selected: selected) else {
            return !nodes.isEmpty
        }
        select(nodes[index])
        return true
    }

    // MARK: - Cells

    private func configure(_ cell: FileTreeRowCell, path: String) {
        guard let node = nodesByPath[path] else { return }
        let section = self.section
        cell.isEmphasized = outlineView.isFirstResponder
        cell.onToggleExpansion = { [weak self, weak node] in
            guard let self, let node else { return }
            self.outlineView.becomeFirstResponder()
            self.toggleExpansion(node)
        }
        cell.configure(
            node: node, level: section.contains(path) ? section.level(of: path) : 0, isExpanded: isExpanded(node))
        cell.accessibilityCustomActions =
            node.isExpandable
            ? [
                UIAccessibilityCustomAction(name: isExpanded(node) ? "Collapse" : "Expand") { [weak self, weak node] _ in
                    guard let self, let node else { return false }
                    self.toggleExpansion(node)
                    return true
                }
            ] : nil
    }

    /// The row a context action came from, for anchoring a share sheet on it.
    func sourceRect(forPath path: String) -> CGRect? {
        guard let indexPath = dataSource.indexPath(for: path),
            let attributes = outlineView.layoutAttributesForItem(at: indexPath)
        else { return nil }
        return attributes.frame.insetBy(dx: FileTreeRowCell.rowInset, dy: 0)
    }

    // MARK: - Context Menu

    /// The Mac menu minus Reveal in Finder: Open Local Copy (remote files), Copy Path and
    /// Copy Relative Path.
    static func menuItems(for node: FileTreeNode, isLocal: Bool) -> [ContextItem] {
        var items: [ContextItem] = []
        if !isLocal && !node.isDirectory { items.append(.openLocalCopy) }
        items.append(.copyPath)
        items.append(.copyRelativePath)
        return items
    }

    enum ContextItem: Equatable {
        case openLocalCopy, copyPath, copyRelativePath

        var title: String {
            switch self {
            case .openLocalCopy: "Open Local Copy"
            case .copyPath: "Copy Path"
            case .copyRelativePath: "Copy Relative Path"
            }
        }
    }

    func perform(_ item: ContextItem, on node: FileTreeNode) {
        switch item {
        case .openLocalCopy: onOpenLocalCopy?(node.absolutePath)
        case .copyPath: UIPasteboard.general.string = node.absolutePath
        case .copyRelativePath: UIPasteboard.general.string = node.relativePath
        }
    }
}

// MARK: - UICollectionViewDelegate

extension FileTreeOutlineViewController: UICollectionViewDelegate {
    func collectionView(_ collectionView: UICollectionView, shouldSelectItemAt indexPath: IndexPath) -> Bool {
        outlineView.becomeFirstResponder()
        // Clicking the selected row again changes nothing, as in the Mac outline.
        return collectionView.indexPathsForSelectedItems?.contains(indexPath) != true
    }

    func collectionView(_ collectionView: UICollectionView, shouldDeselectItemAt indexPath: IndexPath) -> Bool {
        // Single selection: only a new selection or a click below the rows clears a row.
        false
    }

    func collectionView(_ collectionView: UICollectionView, didSelectItemAt indexPath: IndexPath) {
        userSelected(indexPath)
    }

    func collectionView(_ collectionView: UICollectionView, willDisplay cell: UICollectionViewCell, forItemAt indexPath: IndexPath) {
        (cell as? FileTreeRowCell)?.isEmphasized = outlineView.isFirstResponder
    }

    func collectionView(
        _ collectionView: UICollectionView, contextMenuConfigurationForItemsAt indexPaths: [IndexPath], point: CGPoint
    ) -> UIContextMenuConfiguration? {
        guard indexPaths.count == 1, let indexPath = indexPaths.first, let node = node(at: indexPath) else { return nil }
        let isLocal = self.isLocal
        return UIContextMenuConfiguration(identifier: node.absolutePath as NSString, previewProvider: nil) {
            [weak self, weak node] _ in
            guard let node else { return nil }
            let actions = Self.menuItems(for: node, isLocal: isLocal).map { item in
                UIAction(title: item.title) { [weak self, weak node] _ in
                    guard let self, let node else { return }
                    self.perform(item, on: node)
                }
            }
            return UIMenu(children: actions)
        }
    }

    func collectionView(
        _ collectionView: UICollectionView,
        contextMenuConfiguration configuration: UIContextMenuConfiguration,
        highlightPreviewForItemAt indexPath: IndexPath
    ) -> UITargetedPreview? {
        rowPreview(at: indexPath)
    }

    func collectionView(
        _ collectionView: UICollectionView,
        contextMenuConfiguration configuration: UIContextMenuConfiguration,
        dismissalPreviewForItemAt indexPath: IndexPath
    ) -> UITargetedPreview? {
        rowPreview(at: indexPath)
    }

    /// The menu lifts the row's rounded frame, as the Mac outline rings the clicked row.
    private func rowPreview(at indexPath: IndexPath) -> UITargetedPreview? {
        guard let cell = outlineView.cellForItem(at: indexPath) as? FileTreeRowCell else { return nil }
        let parameters = UIPreviewParameters()
        parameters.visiblePath = UIBezierPath(roundedRect: cell.rowView.bounds, cornerRadius: 5)
        // The row draws its own fill; the preview's default white would cover the pane behind it.
        parameters.backgroundColor = .clear
        return UITargetedPreview(view: cell.rowView, parameters: parameters)
    }
}

// MARK: - Background clicks

extension FileTreeOutlineViewController: UIGestureRecognizerDelegate {
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
