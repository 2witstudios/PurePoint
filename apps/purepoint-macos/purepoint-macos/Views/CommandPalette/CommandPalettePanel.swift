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
        variants: [AgentVariant] = AgentVariant.allVariants,
        onSelect: @escaping (AgentVariant, String?) -> Void
    ) -> CommandPalettePanel {
        let panel = CommandPalettePanel()
        guard let vc = panel.contentViewController as? CommandPaletteViewController else {
            return panel
        }
        vc.availableVariants = variants
        vc.onSelect = { variant, prompt in
            panel.dismiss()
            onSelect(variant, prompt)
        }
        vc.onDismiss = {
            panel.dismiss()
        }
        panel.showRelativeTo(window: window)
        return panel
    }
}

// MARK: - CommandPaletteViewController

class CommandPaletteViewController: NSViewController, NSTextFieldDelegate, NSTextViewDelegate {

    var onSelect: ((AgentVariant, String?) -> Void)?
    var onDismiss: (() -> Void)?

    private enum Phase {
        case selection
        case prompt(AgentVariant)
    }

    private var phase: Phase = .selection

    // Phase 1 — Selection
    private let searchField = NSTextField()
    private let scrollView = NSScrollView()
    private let tableView = NSTableView()
    var availableVariants: [AgentVariant] = AgentVariant.allVariants
    private var filteredVariants: [AgentVariant] = AgentVariant.allVariants
    private var selectedIndex = 0

    // Phase 2 — Prompt
    private let promptHeader = NSStackView()
    private let promptHeaderIcon = NSImageView()
    private let promptHeaderLabel = NSTextField(labelWithString: "")
    private let promptScrollView = NSScrollView()
    private let promptTextView = NSTextView()
    private let promptHint = NSTextField(labelWithString: "Enter to submit · Shift+Enter for newline")
    private var promptHeightConstraint: NSLayoutConstraint!

    // Shared
    private let containerView = NSVisualEffectView()
    private let separatorView = NSBox()

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

        NSLayoutConstraint.activate([
            promptHeader.topAnchor.constraint(equalTo: containerView.topAnchor, constant: 16),
            promptHeader.leadingAnchor.constraint(equalTo: containerView.leadingAnchor, constant: 16),
            promptHeader.trailingAnchor.constraint(equalTo: containerView.trailingAnchor, constant: -16),

            promptScrollView.topAnchor.constraint(equalTo: promptHeader.bottomAnchor, constant: 16),
            promptScrollView.leadingAnchor.constraint(equalTo: containerView.leadingAnchor, constant: 16),
            promptScrollView.trailingAnchor.constraint(equalTo: containerView.trailingAnchor, constant: -16),
            promptHeightConstraint,
            promptScrollView.heightAnchor.constraint(lessThanOrEqualToConstant: 120),

            promptHint.topAnchor.constraint(equalTo: promptScrollView.bottomAnchor, constant: 12),
            promptHint.centerXAnchor.constraint(equalTo: containerView.centerXAnchor),
        ])
    }

    // MARK: - Phase Transitions

    private func showSelectionPhase() {
        phase = .selection
        filteredVariants = availableVariants
        selectedIndex = 0

        searchField.stringValue = ""
        searchField.isHidden = false
        separatorView.isHidden = false
        scrollView.isHidden = false

        promptHeader.isHidden = true
        promptScrollView.isHidden = true
        promptHint.isHidden = true

        tableView.reloadData()
        updateHighlight()

        resizePanel(rowCount: filteredVariants.count)

        DispatchQueue.main.async { [weak self] in
            self?.view.window?.makeFirstResponder(self?.searchField)
        }
    }

    private func showPromptPhase(variant: AgentVariant) {
        phase = .prompt(variant)

        searchField.isHidden = true
        separatorView.isHidden = true
        scrollView.isHidden = true

        let img = NSImage(systemSymbolName: variant.icon, accessibilityDescription: variant.displayName)
        promptHeaderIcon.image = img
        promptHeaderIcon.contentTintColor = .labelColor
        promptHeaderLabel.stringValue = variant.displayName

        promptTextView.string = ""
        promptTextView.setPlaceholder(variant.promptPlaceholder)

        promptHeader.isHidden = false
        promptScrollView.isHidden = false
        promptHint.isHidden = false

        promptHeightConstraint.constant = 60

        let panelHeight: CGFloat = 170
        if let panel = view.window as? CommandPalettePanel {
            var frame = panel.frame
            let dy = frame.height - panelHeight
            frame.origin.y += dy
            frame.size.height = panelHeight
            panel.setFrame(frame, display: true, animate: true)
        }

        DispatchQueue.main.async { [weak self] in
            self?.view.window?.makeFirstResponder(self?.promptTextView)
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

    private func filterVariants(query: String) {
        let q = query.lowercased().trimmingCharacters(in: .whitespaces)
        if q.isEmpty {
            filteredVariants = availableVariants
        } else {
            filteredVariants = availableVariants.filter {
                $0.displayName.lowercased().contains(q) ||
                $0.subtitle.lowercased().contains(q) ||
                $0.id.lowercased().contains(q)
            }
        }
        selectedIndex = filteredVariants.isEmpty ? -1 : 0
        tableView.reloadData()
        updateHighlight()
        resizePanel(rowCount: filteredVariants.count)
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

    private func selectCurrentVariant() {
        guard selectedIndex >= 0, selectedIndex < filteredVariants.count else { return }
        let variant = filteredVariants[selectedIndex]
        showPromptPhase(variant: variant)
    }

    private func submitPrompt() {
        guard case .prompt(let variant) = phase else { return }
        let prompt = promptTextView.string.trimmingCharacters(in: .whitespacesAndNewlines)
        onSelect?(variant, prompt.isEmpty ? nil : prompt)
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
        guard row >= 0, row < filteredVariants.count else { return }
        selectedIndex = row
        updateHighlight()
        selectCurrentVariant()
    }

    // MARK: - NSTextFieldDelegate

    func controlTextDidChange(_ obj: Notification) {
        guard let field = obj.object as? NSTextField, field === searchField else { return }
        filterVariants(query: field.stringValue)
    }

    func control(_ control: NSControl, textView: NSTextView, doCommandBy commandSelector: Selector) -> Bool {
        guard control === searchField else { return false }
        return handleSearchFieldCommand(commandSelector)
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
            if selectedIndex < filteredVariants.count - 1 {
                selectedIndex += 1
                updateHighlight()
                tableView.scrollRowToVisible(selectedIndex)
            }
            return true
        case #selector(NSResponder.insertNewline(_:)):
            selectCurrentVariant()
            return true
        case #selector(NSResponder.cancelOperation(_:)):
            handleEscape()
            return true
        default:
            return false
        }
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
        filteredVariants.count
    }

    func tableView(_ tableView: NSTableView, viewFor tableColumn: NSTableColumn?, row: Int) -> NSView? {
        let variant = filteredVariants[row]

        let highlightColor = NSColor.controlAccentColor.withAlphaComponent(0.15).cgColor
        let clearColor = NSColor.clear.cgColor

        let cellView = NSTableCellView()
        cellView.wantsLayer = true
        cellView.layer?.backgroundColor = row == selectedIndex ? highlightColor : clearColor
        cellView.layer?.cornerRadius = 6

        let iconView = NSImageView()
        iconView.image = NSImage(systemSymbolName: variant.icon, accessibilityDescription: variant.displayName)
        iconView.contentTintColor = .labelColor
        iconView.imageScaling = .scaleProportionallyDown
        iconView.translatesAutoresizingMaskIntoConstraints = false

        let nameLabel = NSTextField(labelWithString: variant.displayName)
        nameLabel.font = .boldSystemFont(ofSize: 14)
        nameLabel.textColor = .labelColor
        nameLabel.drawsBackground = false
        nameLabel.isBordered = false

        let subtitleLabel = NSTextField(labelWithString: variant.subtitle)
        subtitleLabel.font = .systemFont(ofSize: 12)
        subtitleLabel.textColor = .secondaryLabelColor
        subtitleLabel.drawsBackground = false
        subtitleLabel.isBordered = false

        let textStack = NSStackView(views: [nameLabel, subtitleLabel])
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
