import AppKit

// MARK: - CommandPalettePanel

class CommandPalettePanel: NSPanel {
    private var localMouseMonitor: Any?

    override var canBecomeKey: Bool { true }

    init() {
        super.init(
            contentRect: NSRect(x: 0, y: 0, width: 500, height: 380),
            styleMask: [.borderless, .nonactivatingPanel],
            backing: .buffered,
            defer: false
        )
        isFloatingPanel = true
        level = .floating
        isOpaque = false
        backgroundColor = .clear
        hasShadow = true

        contentViewController = CommandPaletteViewController()
    }

    func showRelativeTo(window: NSWindow?) {
        guard let parentWindow = window else { return }
        appearance = parentWindow.effectiveAppearance

        let parentFrame = parentWindow.frame
        let panelSize = frame.size
        let x = parentFrame.midX - panelSize.width / 2
        let y = parentFrame.midY - panelSize.height / 2 + 60
        setFrameOrigin(NSPoint(x: x, y: y))

        parentWindow.addChildWindow(self, ordered: .above)
        makeKeyAndOrderFront(nil)

        localMouseMonitor = NSEvent.addLocalMonitorForEvents(matching: [.leftMouseDown, .rightMouseDown]) { [weak self] event in
            guard let self else { return event }
            if event.window !== self {
                self.dismiss()
                return nil
            }
            return event
        }
    }

    func dismiss() {
        if let monitor = localMouseMonitor {
            NSEvent.removeMonitor(monitor)
            localMouseMonitor = nil
        }
        parent?.removeChildWindow(self)
        orderOut(nil)
    }

    deinit {
        if let monitor = localMouseMonitor {
            NSEvent.removeMonitor(monitor)
        }
    }

    override func cancelOperation(_ sender: Any?) {
        guard let vc = contentViewController as? CommandPaletteViewController else {
            dismiss()
            return
        }
        vc.handleEscape()
    }

    @discardableResult
    static func show(
        relativeTo window: NSWindow?,
        items: [CommandPaletteItem] = CommandPaletteItem.buildItems(builtInVariants: AgentVariant.allVariants, agents: [], swarms: []),
        onSelect: @escaping (CommandPaletteResult) -> Void
    ) -> CommandPalettePanel {
        let panel = CommandPalettePanel()
        guard let vc = panel.contentViewController as? CommandPaletteViewController else {
            return panel
        }
        vc.onSelect = { result in
            panel.dismiss()
            onSelect(result)
        }
        vc.onDismiss = {
            panel.dismiss()
        }
        vc.setItems(items)
        panel.showRelativeTo(window: window)
        return panel
    }
}

// MARK: - CommandPaletteViewController

class CommandPaletteViewController: NSViewController, NSTextFieldDelegate, NSTextViewDelegate {

    var onSelect: ((CommandPaletteResult) -> Void)?
    var onDismiss: (() -> Void)?

    private enum Phase {
        case selection
        case prompt(CommandPaletteItem)
    }

    private var phase: Phase = .selection

    // Phase 1 — Selection
    private let searchField = NSTextField()
    private let scrollView = NSScrollView()
    private let tableView = NSTableView()
    var availableItems: [CommandPaletteItem] = []
    private var filteredItems: [CommandPaletteItem] = []
    private var selectedIndex = 0

    // Phase 2 — Prompt
    private let promptHeader = NSStackView()
    private let promptHeaderIcon = NSImageView()
    private let promptHeaderLabel = NSTextField(labelWithString: "")
    private let nameLabel = NSTextField(labelWithString: "Name")
    private let nameField = NSTextField()
    private let slugPreview = NSTextField(labelWithString: "")
    private let promptScrollView = NSScrollView()
    private let promptTextView = NSTextView()
    private let promptHint = NSTextField(labelWithString: "Enter to submit \u{00B7} Shift+Enter for newline")
    private var promptHeightConstraint: NSLayoutConstraint!
    private var nameFieldConstraints: [NSLayoutConstraint] = []

    // Shared
    private let containerView = NSVisualEffectView()
    private let separatorView = NSBox()
    private var promptScrollTopConstraint: NSLayoutConstraint?

    override func loadView() {
        let wrapper = NSView(frame: NSRect(x: 0, y: 0, width: 500, height: 380))
        self.view = wrapper

        containerView.material = .popover
        containerView.blendingMode = .behindWindow
        containerView.state = .active
        containerView.wantsLayer = true
        containerView.layer?.cornerRadius = 12
        containerView.layer?.masksToBounds = true

        containerView.frame = wrapper.bounds
        containerView.autoresizingMask = [.width, .height]
        wrapper.addSubview(containerView)

        setupSearchField()
        setupSeparator()
        setupTableView()
        setupPromptViews()

        showSelectionPhase()
    }

    override func viewDidAppear() {
        super.viewDidAppear()
        view.window?.makeFirstResponder(searchField)
    }

    // MARK: - Setup

    private func setupSearchField() {
        searchField.placeholderString = "Type to filter..."
        searchField.isBordered = false
        searchField.isBezeled = false
        searchField.focusRingType = .none
        searchField.drawsBackground = false
        searchField.textColor = .labelColor
        searchField.font = .systemFont(ofSize: 16)
        searchField.delegate = self
        searchField.translatesAutoresizingMaskIntoConstraints = false
        containerView.addSubview(searchField)

        NSLayoutConstraint.activate([
            searchField.topAnchor.constraint(equalTo: containerView.topAnchor, constant: 16),
            searchField.leadingAnchor.constraint(equalTo: containerView.leadingAnchor, constant: 16),
            searchField.trailingAnchor.constraint(equalTo: containerView.trailingAnchor, constant: -16),
            searchField.heightAnchor.constraint(equalToConstant: 24),
        ])
    }

    private func setupSeparator() {
        separatorView.boxType = .separator
        separatorView.translatesAutoresizingMaskIntoConstraints = false
        containerView.addSubview(separatorView)

        NSLayoutConstraint.activate([
            separatorView.topAnchor.constraint(equalTo: searchField.bottomAnchor, constant: 12),
            separatorView.leadingAnchor.constraint(equalTo: containerView.leadingAnchor),
            separatorView.trailingAnchor.constraint(equalTo: containerView.trailingAnchor),
            separatorView.heightAnchor.constraint(equalToConstant: 1),
        ])
    }

    private func setupTableView() {
        let column = NSTableColumn(identifier: NSUserInterfaceItemIdentifier("variant"))
        column.width = 468
        tableView.addTableColumn(column)
        tableView.headerView = nil
        tableView.backgroundColor = .clear
        tableView.rowHeight = 44
        tableView.intercellSpacing = NSSize(width: 0, height: 0)
        tableView.selectionHighlightStyle = .none
        tableView.dataSource = self
        tableView.delegate = self
        tableView.target = self
        tableView.doubleAction = #selector(tableViewDoubleClick)

        scrollView.documentView = tableView
        scrollView.hasVerticalScroller = false
        scrollView.drawsBackground = false
        scrollView.verticalScrollElasticity = .none
        scrollView.translatesAutoresizingMaskIntoConstraints = false
        containerView.addSubview(scrollView)

        NSLayoutConstraint.activate([
            scrollView.topAnchor.constraint(equalTo: separatorView.bottomAnchor, constant: 4),
            scrollView.leadingAnchor.constraint(equalTo: containerView.leadingAnchor),
            scrollView.trailingAnchor.constraint(equalTo: containerView.trailingAnchor),
            scrollView.bottomAnchor.constraint(equalTo: containerView.bottomAnchor, constant: -8),
        ])
    }

    private func setupPromptViews() {
        promptHeaderIcon.imageScaling = .scaleProportionallyDown
        promptHeaderIcon.translatesAutoresizingMaskIntoConstraints = false
        promptHeaderIcon.setContentHuggingPriority(.required, for: .horizontal)
        NSLayoutConstraint.activate([
            promptHeaderIcon.widthAnchor.constraint(equalToConstant: 18),
            promptHeaderIcon.heightAnchor.constraint(equalToConstant: 18),
        ])

        promptHeaderLabel.font = .boldSystemFont(ofSize: 16)
        promptHeaderLabel.textColor = .labelColor
        promptHeaderLabel.isEditable = false
        promptHeaderLabel.isBordered = false
        promptHeaderLabel.drawsBackground = false

        promptHeader.orientation = .horizontal
        promptHeader.spacing = 8
        promptHeader.alignment = .centerY
        promptHeader.addArrangedSubview(promptHeaderIcon)
        promptHeader.addArrangedSubview(promptHeaderLabel)
        promptHeader.translatesAutoresizingMaskIntoConstraints = false
        containerView.addSubview(promptHeader)

        nameLabel.font = .systemFont(ofSize: 12, weight: .medium)
        nameLabel.textColor = .secondaryLabelColor
        nameLabel.translatesAutoresizingMaskIntoConstraints = false
        containerView.addSubview(nameLabel)

        nameField.placeholderString = "e.g., fix-auth-bug"
        nameField.isBordered = true
        nameField.isBezeled = true
        nameField.bezelStyle = .roundedBezel
        nameField.focusRingType = .default
        nameField.font = .systemFont(ofSize: 14)
        nameField.textColor = .labelColor
        nameField.delegate = self
        nameField.translatesAutoresizingMaskIntoConstraints = false
        containerView.addSubview(nameField)

        slugPreview.font = .monospacedSystemFont(ofSize: 11, weight: .regular)
        slugPreview.textColor = .tertiaryLabelColor
        slugPreview.translatesAutoresizingMaskIntoConstraints = false
        containerView.addSubview(slugPreview)

        promptTextView.isEditable = true
        promptTextView.isRichText = false
        promptTextView.allowsUndo = true
        promptTextView.font = .systemFont(ofSize: 16)
        promptTextView.textColor = .labelColor
        promptTextView.backgroundColor = .textBackgroundColor
        promptTextView.isVerticallyResizable = true
        promptTextView.isHorizontallyResizable = false
        promptTextView.textContainer?.widthTracksTextView = true
        promptTextView.textContainer?.lineFragmentPadding = 8
        promptTextView.textContainerInset = NSSize(width: 0, height: 6)
        promptTextView.delegate = self
        promptTextView.isAutomaticQuoteSubstitutionEnabled = false
        promptTextView.isAutomaticDashSubstitutionEnabled = false
        promptTextView.isAutomaticTextReplacementEnabled = false

        promptScrollView.documentView = promptTextView
        promptScrollView.hasVerticalScroller = true
        promptScrollView.hasHorizontalScroller = false
        promptScrollView.autohidesScrollers = true
        promptScrollView.drawsBackground = true
        promptScrollView.backgroundColor = .textBackgroundColor
        promptScrollView.borderType = .bezelBorder
        promptScrollView.translatesAutoresizingMaskIntoConstraints = false
        containerView.addSubview(promptScrollView)

        promptHint.font = .systemFont(ofSize: 12)
        promptHint.textColor = .secondaryLabelColor
        promptHint.alignment = .center
        promptHint.translatesAutoresizingMaskIntoConstraints = false
        containerView.addSubview(promptHint)

        promptHeightConstraint = promptScrollView.heightAnchor.constraint(equalToConstant: 60)

        nameFieldConstraints = [
            nameLabel.topAnchor.constraint(equalTo: promptHeader.bottomAnchor, constant: 12),
            nameLabel.leadingAnchor.constraint(equalTo: containerView.leadingAnchor, constant: 16),

            nameField.topAnchor.constraint(equalTo: nameLabel.bottomAnchor, constant: 4),
            nameField.leadingAnchor.constraint(equalTo: containerView.leadingAnchor, constant: 16),
            nameField.trailingAnchor.constraint(equalTo: containerView.trailingAnchor, constant: -16),
            nameField.heightAnchor.constraint(equalToConstant: 24),

            slugPreview.topAnchor.constraint(equalTo: nameField.bottomAnchor, constant: 4),
            slugPreview.leadingAnchor.constraint(equalTo: containerView.leadingAnchor, constant: 16),
            slugPreview.trailingAnchor.constraint(equalTo: containerView.trailingAnchor, constant: -16),
        ]

        NSLayoutConstraint.activate([
            promptHeader.topAnchor.constraint(equalTo: containerView.topAnchor, constant: 16),
            promptHeader.leadingAnchor.constraint(equalTo: containerView.leadingAnchor, constant: 16),
            promptHeader.trailingAnchor.constraint(equalTo: containerView.trailingAnchor, constant: -16),

            promptScrollView.leadingAnchor.constraint(equalTo: containerView.leadingAnchor, constant: 16),
            promptScrollView.trailingAnchor.constraint(equalTo: containerView.trailingAnchor, constant: -16),
            promptHeightConstraint,
            promptScrollView.heightAnchor.constraint(lessThanOrEqualToConstant: 120),

            promptHint.topAnchor.constraint(equalTo: promptScrollView.bottomAnchor, constant: 12),
            promptHint.centerXAnchor.constraint(equalTo: containerView.centerXAnchor),
        ])
    }

    // MARK: - Configuration

    func setItems(_ items: [CommandPaletteItem]) {
        availableItems = items
        filteredItems = items
        selectedIndex = 0
        tableView.reloadData()
        updateHighlight()
        resizePanel(rowCount: filteredItems.count)
    }

    // MARK: - Phase Transitions

    private func showSelectionPhase() {
        phase = .selection
        filteredItems = availableItems
        selectedIndex = 0

        searchField.stringValue = ""
        searchField.isHidden = false
        separatorView.isHidden = false
        scrollView.isHidden = false

        promptHeader.isHidden = true
        promptScrollView.isHidden = true
        promptHint.isHidden = true
        nameLabel.isHidden = true
        nameField.isHidden = true
        slugPreview.isHidden = true
        NSLayoutConstraint.deactivate(nameFieldConstraints)
        promptScrollTopConstraint?.isActive = false

        tableView.reloadData()
        updateHighlight()

        resizePanel(rowCount: filteredItems.count)

        DispatchQueue.main.async { [weak self] in
            self?.view.window?.makeFirstResponder(self?.searchField)
        }
    }

    private func showPromptPhase(item: CommandPaletteItem) {
        phase = .prompt(item)

        searchField.isHidden = true
        separatorView.isHidden = true
        scrollView.isHidden = true

        let img = NSImage(systemSymbolName: item.icon, accessibilityDescription: item.displayName)
        promptHeaderIcon.image = img
        promptHeaderIcon.contentTintColor = .labelColor
        promptHeaderLabel.stringValue = item.displayName

        promptTextView.string = ""
        promptTextView.setPlaceholder(item.promptPlaceholder)

        let showName = item.showsNameField
        nameLabel.isHidden = !showName
        nameField.isHidden = !showName
        slugPreview.isHidden = !showName
        nameField.stringValue = ""
        slugPreview.stringValue = ""

        // Swap the dynamic top constraint for promptScrollView
        promptScrollTopConstraint?.isActive = false
        if showName {
            NSLayoutConstraint.activate(nameFieldConstraints)
            promptScrollTopConstraint = promptScrollView.topAnchor.constraint(equalTo: slugPreview.bottomAnchor, constant: 12)
        } else {
            NSLayoutConstraint.deactivate(nameFieldConstraints)
            promptScrollTopConstraint = promptScrollView.topAnchor.constraint(equalTo: promptHeader.bottomAnchor, constant: 16)
        }
        promptScrollTopConstraint?.isActive = true

        promptHeader.isHidden = false
        promptScrollView.isHidden = false
        promptHint.isHidden = false

        promptHeightConstraint.constant = 60

        let panelHeight: CGFloat = showName ? 260 : 170
        if let panel = view.window as? CommandPalettePanel {
            var frame = panel.frame
            let dy = frame.height - panelHeight
            frame.origin.y += dy
            frame.size.height = panelHeight
            panel.setFrame(frame, display: true, animate: true)
        }

        DispatchQueue.main.async { [weak self] in
            if showName {
                self?.view.window?.makeFirstResponder(self?.nameField)
            } else {
                self?.view.window?.makeFirstResponder(self?.promptTextView)
            }
        }
    }

    private func resizePanel(rowCount: Int) {
        let headerHeight: CGFloat = 56
        let rowsHeight = CGFloat(max(rowCount, 1)) * 44
        let bottomPadding: CGFloat = 8
        let panelHeight = min(headerHeight + rowsHeight + bottomPadding, 380)

        if let panel = view.window as? CommandPalettePanel {
            var frame = panel.frame
            let dy = frame.height - panelHeight
            frame.origin.y += dy
            frame.size.height = panelHeight
            panel.setFrame(frame, display: true, animate: true)
        }
    }

    // MARK: - Filtering

    private func filterItems(query: String) {
        let q = query.lowercased().trimmingCharacters(in: .whitespaces)
        if q.isEmpty {
            filteredItems = availableItems
        } else {
            filteredItems = availableItems
                .compactMap { item -> (CommandPaletteItem, Int)? in
                    let text = item.searchableText.lowercased()
                    let name = item.displayName.lowercased()

                    let score: Int
                    if name.hasPrefix(q) {
                        score = 100
                    } else if name.contains(q) {
                        score = 50
                    } else if text.contains(q) {
                        score = 10
                    } else {
                        return nil
                    }
                    return (item, score)
                }
                .sorted { $0.1 > $1.1 }
                .map(\.0)
        }
        selectedIndex = filteredItems.isEmpty ? -1 : 0
        tableView.reloadData()
        updateHighlight()
        resizePanel(rowCount: filteredItems.count)
    }

    // MARK: - Highlight

    private func updateHighlight() {
        let highlightColor = NSColor.controlAccentColor.withAlphaComponent(0.15).cgColor
        let clearColor = NSColor.clear.cgColor

        for row in 0..<tableView.numberOfRows {
            if let cellView = tableView.view(atColumn: 0, row: row, makeIfNecessary: false) {
                cellView.layer?.backgroundColor = row == selectedIndex
                    ? highlightColor
                    : clearColor
            }
        }
    }

    // MARK: - Actions

    private func selectCurrentItem() {
        guard selectedIndex >= 0, selectedIndex < filteredItems.count else { return }
        let item = filteredItems[selectedIndex]
        if item.skipsPromptPhase {
            let result: CommandPaletteResult
            switch item {
            case .agentDef(let d): result = .spawnAgentDef(def: d, prompt: nil)
            case .swarm(let s): result = .runSwarm(def: s)
            case .builtIn: return
            }
            onSelect?(result)
        } else {
            showPromptPhase(item: item)
        }
    }

    private func submitPrompt() {
        guard case .prompt(let item) = phase else { return }
        let prompt = promptTextView.string.trimmingCharacters(in: .whitespacesAndNewlines)
        let name = nameField.isHidden ? nil : nameField.stringValue.trimmingCharacters(in: .whitespacesAndNewlines)
        let promptOrNil = prompt.isEmpty ? nil : prompt
        let nameOrNil = name?.isEmpty == true ? nil : name

        let result: CommandPaletteResult
        switch item {
        case .builtIn(let v): result = .spawnBuiltIn(variant: v, prompt: promptOrNil, name: nameOrNil)
        case .agentDef(let d): result = .spawnAgentDef(def: d, prompt: promptOrNil)
        case .swarm(let s): result = .runSwarm(def: s)
        }
        onSelect?(result)
    }

    func handleEscape() {
        switch phase {
        case .selection:
            onDismiss?()
        case .prompt:
            showSelectionPhase()
        }
    }

    @objc private func tableViewDoubleClick() {
        let row = tableView.clickedRow
        guard row >= 0, row < filteredItems.count else { return }
        selectedIndex = row
        updateHighlight()
        selectCurrentItem()
    }

    // MARK: - NSTextFieldDelegate

    func controlTextDidChange(_ obj: Notification) {
        guard let field = obj.object as? NSTextField else { return }
        if field === searchField {
            filterItems(query: field.stringValue)
        } else if field === nameField {
            updateSlugPreview()
        }
    }

    func control(_ control: NSControl, textView: NSTextView, doCommandBy commandSelector: Selector) -> Bool {
        if control === searchField {
            return handleSearchFieldCommand(commandSelector)
        }
        if control === nameField {
            return handleNameFieldCommand(commandSelector)
        }
        return false
    }

    // MARK: - NSTextViewDelegate

    func textView(_ textView: NSTextView, doCommandBy commandSelector: Selector) -> Bool {
        guard textView === promptTextView else { return false }
        if commandSelector == #selector(NSResponder.insertNewline(_:)) {
            if NSEvent.modifierFlags.contains(.shift) || NSEvent.modifierFlags.contains(.option) {
                textView.insertNewlineIgnoringFieldEditor(nil)
                return true
            }
            submitPrompt()
            return true
        }
        if commandSelector == #selector(NSResponder.cancelOperation(_:)) {
            handleEscape()
            return true
        }
        return false
    }

    func textDidChange(_ notification: Notification) {
        guard let textView = notification.object as? NSTextView, textView === promptTextView else { return }
        adjustPromptHeight()
    }

    private func handleSearchFieldCommand(_ sel: Selector) -> Bool {
        switch sel {
        case #selector(NSResponder.moveUp(_:)):
            if selectedIndex > 0 {
                selectedIndex -= 1
                updateHighlight()
                tableView.scrollRowToVisible(selectedIndex)
            }
            return true
        case #selector(NSResponder.moveDown(_:)):
            if selectedIndex < filteredItems.count - 1 {
                selectedIndex += 1
                updateHighlight()
                tableView.scrollRowToVisible(selectedIndex)
            }
            return true
        case #selector(NSResponder.insertNewline(_:)):
            selectCurrentItem()
            return true
        case #selector(NSResponder.cancelOperation(_:)):
            handleEscape()
            return true
        default:
            return false
        }
    }

    private func handleNameFieldCommand(_ sel: Selector) -> Bool {
        switch sel {
        case #selector(NSResponder.insertNewline(_:)),
             #selector(NSResponder.insertTab(_:)):
            view.window?.makeFirstResponder(promptTextView)
            return true
        case #selector(NSResponder.cancelOperation(_:)):
            handleEscape()
            return true
        default:
            return false
        }
    }

    private func updateSlugPreview() {
        let raw = nameField.stringValue
        let slug = Self.normalizeWorktreeName(raw)
        slugPreview.stringValue = slug.isEmpty ? "" : "pu/\(slug)"
    }

    static func normalizeWorktreeName(_ input: String) -> String {
        let lowered = input.lowercased()
        var result = ""
        for ch in lowered {
            if ch.isASCII && (ch.isLetter || ch.isNumber) {
                result.append(ch)
            } else if ch.isWhitespace || ch == "_" {
                result.append("-")
            }
        }
        // Collapse consecutive hyphens
        while result.contains("--") {
            result = result.replacingOccurrences(of: "--", with: "-")
        }
        // Trim leading/trailing hyphens
        return result.trimmingCharacters(in: CharacterSet(charactersIn: "-"))
    }

    private func adjustPromptHeight() {
        guard let layoutManager = promptTextView.layoutManager,
              let textContainer = promptTextView.textContainer else { return }
        layoutManager.ensureLayout(for: textContainer)
        let textHeight = layoutManager.usedRect(for: textContainer).height
        let insets = promptTextView.textContainerInset
        let newHeight = min(max(textHeight + insets.height * 2 + 4, 60), 120)
        promptHeightConstraint.constant = newHeight

        let panelHeight: CGFloat = 16 + 22 + 16 + newHeight + 12 + 16 + 16
        if let panel = view.window as? CommandPalettePanel {
            var frame = panel.frame
            let dy = frame.height - panelHeight
            frame.origin.y += dy
            frame.size.height = panelHeight
            panel.setFrame(frame, display: true)
        }
    }
}

// MARK: - NSTableViewDataSource & Delegate

extension CommandPaletteViewController: NSTableViewDataSource, NSTableViewDelegate {

    func numberOfRows(in tableView: NSTableView) -> Int {
        filteredItems.count
    }

    func tableView(_ tableView: NSTableView, viewFor tableColumn: NSTableColumn?, row: Int) -> NSView? {
        let item = filteredItems[row]

        let highlightColor = NSColor.controlAccentColor.withAlphaComponent(0.15).cgColor
        let clearColor = NSColor.clear.cgColor

        let cellView = NSTableCellView()
        cellView.wantsLayer = true
        cellView.layer?.backgroundColor = row == selectedIndex ? highlightColor : clearColor
        cellView.layer?.cornerRadius = 6

        let iconView = NSImageView()
        iconView.image = NSImage(systemSymbolName: item.icon, accessibilityDescription: item.displayName)
        iconView.contentTintColor = .labelColor
        iconView.imageScaling = .scaleProportionallyDown
        iconView.translatesAutoresizingMaskIntoConstraints = false

        let nameLabel = NSTextField(labelWithString: item.displayName)
        nameLabel.font = .boldSystemFont(ofSize: 14)
        nameLabel.textColor = .labelColor
        nameLabel.drawsBackground = false
        nameLabel.isBordered = false

        let subtitleLabel = NSTextField(labelWithString: item.subtitle)
        subtitleLabel.font = .systemFont(ofSize: 12)
        subtitleLabel.textColor = .secondaryLabelColor
        subtitleLabel.drawsBackground = false
        subtitleLabel.isBordered = false

        var textViews: [NSView] = [nameLabel, subtitleLabel]

        // Category badge for non-built-in items
        if let category = item.categoryLabel {
            let badge = NSTextField(labelWithString: category)
            badge.font = .systemFont(ofSize: 10, weight: .medium)
            badge.textColor = .tertiaryLabelColor
            badge.drawsBackground = false
            badge.isBordered = false
            textViews.append(badge)
        }

        let textStack = NSStackView(views: textViews)
        textStack.orientation = .horizontal
        textStack.spacing = 8
        textStack.alignment = .firstBaseline
        textStack.translatesAutoresizingMaskIntoConstraints = false

        cellView.addSubview(iconView)
        cellView.addSubview(textStack)

        NSLayoutConstraint.activate([
            iconView.leadingAnchor.constraint(equalTo: cellView.leadingAnchor, constant: 16),
            iconView.centerYAnchor.constraint(equalTo: cellView.centerYAnchor),
            iconView.widthAnchor.constraint(equalToConstant: 20),
            iconView.heightAnchor.constraint(equalToConstant: 20),

            textStack.leadingAnchor.constraint(equalTo: iconView.trailingAnchor, constant: 12),
            textStack.centerYAnchor.constraint(equalTo: cellView.centerYAnchor),
            textStack.trailingAnchor.constraint(lessThanOrEqualTo: cellView.trailingAnchor, constant: -16),
        ])

        return cellView
    }

    func tableView(_ tableView: NSTableView, shouldSelectRow row: Int) -> Bool {
        selectedIndex = row
        updateHighlight()
        return false
    }

    func tableViewSelectionDidChange(_ notification: Notification) {}
}

// MARK: - NSTextView Placeholder Helper

private extension NSTextView {
    func setPlaceholder(_ text: String) {
        let attrs: [NSAttributedString.Key: Any] = [
            .foregroundColor: NSColor.placeholderTextColor,
            .font: font ?? NSFont.systemFont(ofSize: 16),
        ]
        setValue(NSAttributedString(string: text, attributes: attrs),
                forKey: "placeholderAttributedString")
    }
}
