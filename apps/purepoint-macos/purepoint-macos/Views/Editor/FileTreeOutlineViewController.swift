import AppKit

@MainActor
class FileTreeOutlineViewController: NSViewController, NSOutlineViewDataSource, NSOutlineViewDelegate {

    let scrollView = NSScrollView()
    let outlineView = NSOutlineView()

    var rootNodes: [FileTreeNode] = []

    var onFileSelected: ((String, String) -> Void)?  // (absolutePath, name)
    var onNodeExpanded: ((FileTreeNode) -> Void)?
    var onNodeCollapsed: ((FileTreeNode) -> Void)?

    private var contextClickedNode: FileTreeNode?

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

    // MARK: - Data Update

    func updateNodes(_ nodes: [FileTreeNode]) {
        let scrollOrigin = scrollView.contentView.bounds.origin
        rootNodes = nodes
        outlineView.reloadData()
        restoreExpansionState()
        scrollView.contentView.scroll(to: scrollOrigin)
        scrollView.reflectScrolledClipView(scrollView.contentView)
    }

    private var expandedPaths: Set<String> = []

    private func restoreExpansionState() {
        func expandMatching(in nodes: [FileTreeNode]) {
            for node in nodes where node.isDirectory && expandedPaths.contains(node.absolutePath) {
                outlineView.expandItem(node)
                expandMatching(in: node.children)
            }
        }
        expandMatching(in: rootNodes)
    }

    // MARK: - NSOutlineViewDataSource

    func outlineView(_ outlineView: NSOutlineView, numberOfChildrenOfItem item: Any?) -> Int {
        if item == nil { return rootNodes.count }
        if let node = item as? FileTreeNode { return node.children.count }
        return 0
    }

    func outlineView(_ outlineView: NSOutlineView, child index: Int, ofItem item: Any?) -> Any {
        if item == nil { return rootNodes[index] }
        if let node = item as? FileTreeNode { return node.children[index] }
        fatalError("Unexpected item type")
    }

    func outlineView(_ outlineView: NSOutlineView, isItemExpandable item: Any) -> Bool {
        guard let node = item as? FileTreeNode else { return false }
        return node.isExpandable
    }

    // MARK: - NSOutlineViewDelegate

    func outlineView(_ outlineView: NSOutlineView, viewFor tableColumn: NSTableColumn?, item: Any) -> NSView? {
        guard let node = item as? FileTreeNode else { return nil }
        return makeCell(for: node)
    }

    func outlineView(_ outlineView: NSOutlineView, heightOfRowByItem item: Any) -> CGFloat {
        24
    }

    func outlineViewSelectionDidChange(_ notification: Notification) {
        let row = outlineView.selectedRow
        guard row >= 0, let node = outlineView.item(atRow: row) as? FileTreeNode else { return }

        if !node.isDirectory {
            onFileSelected?(node.absolutePath, node.name)
        }
    }

    func outlineViewItemDidExpand(_ notification: Notification) {
        guard let node = notification.userInfo?["NSObject"] as? FileTreeNode else { return }
        expandedPaths.insert(node.absolutePath)
        onNodeExpanded?(node)
        // Reload children after expansion to show lazy-loaded data
        outlineView.reloadItem(node, reloadChildren: true)
    }

    func outlineViewItemWillCollapse(_ notification: Notification) {
        guard let node = notification.userInfo?["NSObject"] as? FileTreeNode else { return }
        expandedPaths.remove(node.absolutePath)
        onNodeCollapsed?(node)
    }

    // MARK: - Cell Factory

    private func makeCell(for node: FileTreeNode) -> NSView {
        let cell = NSTableCellView()
        let stack = NSStackView()
        stack.orientation = .horizontal
        stack.spacing = 5
        stack.translatesAutoresizingMaskIntoConstraints = false
        cell.addSubview(stack)

        NSLayoutConstraint.activate([
            stack.leadingAnchor.constraint(equalTo: cell.leadingAnchor, constant: 2),
            stack.trailingAnchor.constraint(equalTo: cell.trailingAnchor, constant: -4),
            stack.centerYAnchor.constraint(equalTo: cell.centerYAnchor),
        ])

        let iconName = node.isDirectory ? "folder.fill" : fileIcon(for: node.name)
        let icon = NSImageView(
            image: NSImage(systemSymbolName: iconName, accessibilityDescription: node.isDirectory ? "Folder" : "File")!)
        icon.contentTintColor = node.isDirectory ? .systemBlue : .secondaryLabelColor
        icon.symbolConfiguration = NSImage.SymbolConfiguration(pointSize: 11, weight: .regular)
        icon.setContentHuggingPriority(.required, for: .horizontal)

        let label = NSTextField(labelWithString: node.name)
        label.font = .systemFont(ofSize: 12)
        label.lineBreakMode = .byTruncatingTail

        stack.addArrangedSubview(icon)
        stack.addArrangedSubview(label)

        return cell
    }

    private func fileIcon(for name: String) -> String {
        let language = EditorLanguage.detect(from: name)
        return language.icon
    }
}

// MARK: - Context Menu

extension FileTreeOutlineViewController: NSMenuDelegate {
    func menuNeedsUpdate(_ menu: NSMenu) {
        menu.removeAllItems()

        let clickedRow = outlineView.clickedRow
        guard clickedRow >= 0, let node = outlineView.item(atRow: clickedRow) as? FileTreeNode else { return }
        contextClickedNode = node

        let revealItem = NSMenuItem(
            title: "Reveal in Finder",
            action: #selector(contextRevealInFinder(_:)),
            keyEquivalent: ""
        )
        revealItem.target = self
        menu.addItem(revealItem)

        let copyItem = NSMenuItem(
            title: "Copy Path",
            action: #selector(contextCopyPath(_:)),
            keyEquivalent: ""
        )
        copyItem.target = self
        menu.addItem(copyItem)

        let copyRelItem = NSMenuItem(
            title: "Copy Relative Path",
            action: #selector(contextCopyRelativePath(_:)),
            keyEquivalent: ""
        )
        copyRelItem.target = self
        menu.addItem(copyRelItem)
    }

    @objc private func contextRevealInFinder(_ sender: NSMenuItem) {
        guard let node = contextClickedNode else { return }
        NSWorkspace.shared.selectFile(node.absolutePath, inFileViewerRootedAtPath: "")
    }

    @objc private func contextCopyPath(_ sender: NSMenuItem) {
        guard let node = contextClickedNode else { return }
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(node.absolutePath, forType: .string)
    }

    @objc private func contextCopyRelativePath(_ sender: NSMenuItem) {
        guard let node = contextClickedNode else { return }
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(node.relativePath, forType: .string)
    }
}
