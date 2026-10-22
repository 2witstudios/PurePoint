#if os(iOS)
import UIKit

// MARK: - CommandPaletteViewController

/// iOS twin of the Mac palette content: the search field over the result rows, then the
/// naming and prompt steps, with the same layout metrics, filtering and keys. AppKit's field
/// editor commands become key commands (↑ ↓ Esc Tab) and text field return handling.
class CommandPaletteViewController: UIViewController, UITextFieldDelegate, UITextViewDelegate {

    var onSelect: ((CommandPaletteResult) -> Void)?
    var onDismiss: (() -> Void)?

    private enum Phase {
        case selection
        case naming(CommandPaletteItem)
        case prompt(CommandPaletteItem)
    }

    private var phase: Phase = .selection
    var usePointGuardNames = true
    private var draftItemID: String?

    private func requiresName(_ item: CommandPaletteItem) -> Bool {
        !usePointGuardNames && item.isIndividualLaunch
    }

    // Phase 1 — Selection
    private let searchField = UITextField()
    private let tableView = UITableView(frame: .zero, style: .plain)
    var availableItems: [CommandPaletteItem] = []
    private var filteredItems: [CommandPaletteItem] = []
    private var selectedIndex = 0

    // Phase 2 — Prompt
    private let promptHeader = UIStackView()
    private let promptHeaderIcon = UIImageView()
    private let promptHeaderLabel = UILabel()
    private let nameLabel = UILabel()
    private let nameField = UITextField()
    private let slugPreview = UILabel()
    private let promptTextView = CommandPalettePromptTextView()
    private let promptPlaceholder = UILabel()
    private let promptHint = UILabel()
    private var promptHeightConstraint: NSLayoutConstraint!
    private var nameFieldConstraints: [NSLayoutConstraint] = []

    // Shared
    private let containerView = UIVisualEffectView(effect: UIBlurEffect(style: .systemMaterial))
    private var contentView: UIView { containerView.contentView }
    private let separatorView = UIView()
    private var promptScrollTopConstraint: NSLayoutConstraint?

    override func loadView() {
        let wrapper = UIView(frame: CGRect(x: 0, y: 0, width: 500, height: 380))
        self.view = wrapper

        containerView.layer.cornerRadius = CommandPalettePanel.cornerRadius
        containerView.layer.cornerCurve = .continuous
        containerView.clipsToBounds = true

        containerView.frame = wrapper.bounds
        containerView.autoresizingMask = [.flexibleWidth, .flexibleHeight]
        wrapper.addSubview(containerView)

        setupSearchField()
        setupSeparator()
        setupTableView()
        setupPromptViews()

        showSelectionPhase()
    }

    override func viewDidAppear(_ animated: Bool) {
        super.viewDidAppear(animated)
        focusSearchFieldIfSelecting()
    }

    // MARK: - Setup

    private func setupSearchField() {
        searchField.placeholder = "Type to filter..."
        searchField.borderStyle = .none
        searchField.backgroundColor = .clear
        searchField.textColor = .labelColor
        searchField.font = .systemFont(ofSize: 16)
        searchField.autocorrectionType = .no
        searchField.autocapitalizationType = .none
        searchField.spellCheckingType = .no
        searchField.smartQuotesType = .no
        searchField.smartDashesType = .no
        searchField.delegate = self
        searchField.addTarget(self, action: #selector(textFieldDidChange(_:)), for: .editingChanged)
        searchField.translatesAutoresizingMaskIntoConstraints = false
        contentView.addSubview(searchField)

        NSLayoutConstraint.activate([
            searchField.topAnchor.constraint(equalTo: contentView.topAnchor, constant: 16),
            searchField.leadingAnchor.constraint(equalTo: contentView.leadingAnchor, constant: 16),
            searchField.trailingAnchor.constraint(equalTo: contentView.trailingAnchor, constant: -16),
            searchField.heightAnchor.constraint(equalToConstant: 24),
        ])
    }

    private func setupSeparator() {
        separatorView.backgroundColor = .separatorColor
        separatorView.translatesAutoresizingMaskIntoConstraints = false
        contentView.addSubview(separatorView)

        NSLayoutConstraint.activate([
            separatorView.topAnchor.constraint(equalTo: searchField.bottomAnchor, constant: 12),
            separatorView.leadingAnchor.constraint(equalTo: contentView.leadingAnchor),
            separatorView.trailingAnchor.constraint(equalTo: contentView.trailingAnchor),
            separatorView.heightAnchor.constraint(equalToConstant: 1),
        ])
    }

    private func setupTableView() {
        tableView.register(CommandPaletteRowView.self, forCellReuseIdentifier: CommandPaletteRowView.identifier)
        tableView.backgroundColor = .clear
        tableView.rowHeight = 44
        tableView.estimatedRowHeight = 44
        tableView.separatorStyle = .none
        tableView.sectionHeaderTopPadding = 0
        tableView.allowsSelection = false
        tableView.showsVerticalScrollIndicator = false
        tableView.bounces = false
        tableView.contentInsetAdjustmentBehavior = .never
        tableView.insetsContentViewsToSafeArea = false
        tableView.cellLayoutMarginsFollowReadableWidth = false
        tableView.dataSource = self
        tableView.delegate = self
        tableView.translatesAutoresizingMaskIntoConstraints = false
        contentView.addSubview(tableView)

        // A click highlights a row and a double-click opens it, as in the Mac table; a tap
        // with a finger opens it straight away.
        let tap = UITapGestureRecognizer(target: self, action: #selector(rowTapped(_:)))
        tap.allowedTouchTypes = [UITouch.TouchType.direct.rawValue as NSNumber, UITouch.TouchType.pencil.rawValue as NSNumber]
        let click = UITapGestureRecognizer(target: self, action: #selector(rowClicked(_:)))
        click.allowedTouchTypes = [UITouch.TouchType.indirectPointer.rawValue as NSNumber]
        let doubleClick = UITapGestureRecognizer(target: self, action: #selector(rowDoubleClicked(_:)))
        doubleClick.numberOfTapsRequired = 2
        doubleClick.allowedTouchTypes = [UITouch.TouchType.indirectPointer.rawValue as NSNumber]
        for recognizer in [tap, click, doubleClick] {
            recognizer.delegate = self
            tableView.addGestureRecognizer(recognizer)
        }

        NSLayoutConstraint.activate([
            tableView.topAnchor.constraint(equalTo: separatorView.bottomAnchor, constant: 4),
            tableView.leadingAnchor.constraint(equalTo: contentView.leadingAnchor),
            tableView.trailingAnchor.constraint(equalTo: contentView.trailingAnchor),
            tableView.bottomAnchor.constraint(equalTo: contentView.bottomAnchor, constant: -8),
        ])
    }

    private func setupPromptViews() {
        setupPromptHeader()
        setupNameField()
        setupPromptTextView()

        promptHeightConstraint = promptTextView.heightAnchor.constraint(equalToConstant: 60)

        nameFieldConstraints = [
            nameLabel.topAnchor.constraint(equalTo: promptHeader.bottomAnchor, constant: 12),
            nameLabel.leadingAnchor.constraint(equalTo: contentView.leadingAnchor, constant: 16),

            nameField.topAnchor.constraint(equalTo: nameLabel.bottomAnchor, constant: 4),
            nameField.leadingAnchor.constraint(equalTo: contentView.leadingAnchor, constant: 16),
            nameField.trailingAnchor.constraint(equalTo: contentView.trailingAnchor, constant: -16),
            nameField.heightAnchor.constraint(equalToConstant: 24),

            slugPreview.topAnchor.constraint(equalTo: nameField.bottomAnchor, constant: 4),
            slugPreview.leadingAnchor.constraint(equalTo: contentView.leadingAnchor, constant: 16),
            slugPreview.trailingAnchor.constraint(equalTo: contentView.trailingAnchor, constant: -16),
        ]

        NSLayoutConstraint.activate([
            promptHeader.topAnchor.constraint(equalTo: contentView.topAnchor, constant: 16),
            promptHeader.leadingAnchor.constraint(equalTo: contentView.leadingAnchor, constant: 16),
            promptHeader.trailingAnchor.constraint(equalTo: contentView.trailingAnchor, constant: -16),

            promptTextView.leadingAnchor.constraint(equalTo: contentView.leadingAnchor, constant: 16),
            promptTextView.trailingAnchor.constraint(equalTo: contentView.trailingAnchor, constant: -16),
            promptHeightConstraint,
            promptTextView.heightAnchor.constraint(lessThanOrEqualToConstant: 120),

            promptHint.topAnchor.constraint(equalTo: promptTextView.bottomAnchor, constant: 12),
            promptHint.centerXAnchor.constraint(equalTo: contentView.centerXAnchor),
        ])
    }

    private func setupPromptHeader() {
        promptHeaderIcon.contentMode = .center
        promptHeaderIcon.preferredSymbolConfiguration = UIImage.SymbolConfiguration(pointSize: 13)
        promptHeaderIcon.translatesAutoresizingMaskIntoConstraints = false
        promptHeaderIcon.setContentHuggingPriority(.required, for: .horizontal)
        NSLayoutConstraint.activate([
            promptHeaderIcon.widthAnchor.constraint(equalToConstant: 18),
            promptHeaderIcon.heightAnchor.constraint(equalToConstant: 18),
        ])

        promptHeaderLabel.font = .boldSystemFont(ofSize: 16)
        promptHeaderLabel.textColor = .labelColor

        promptHeader.axis = .horizontal
        promptHeader.spacing = 8
        promptHeader.alignment = .center
        promptHeader.addArrangedSubview(promptHeaderIcon)
        promptHeader.addArrangedSubview(promptHeaderLabel)
        promptHeader.translatesAutoresizingMaskIntoConstraints = false
        contentView.addSubview(promptHeader)
    }

    private func setupNameField() {
        nameLabel.text = "Name"
        nameLabel.font = .systemFont(ofSize: 12, weight: .medium)
        nameLabel.textColor = .secondaryLabelColor
        nameLabel.translatesAutoresizingMaskIntoConstraints = false
        contentView.addSubview(nameLabel)

        nameField.placeholder = "e.g., fix-auth-bug"
        nameField.borderStyle = .roundedRect
        nameField.font = .systemFont(ofSize: 14)
        nameField.textColor = .labelColor
        nameField.autocorrectionType = .no
        nameField.autocapitalizationType = .none
        nameField.spellCheckingType = .no
        nameField.smartQuotesType = .no
        nameField.smartDashesType = .no
        nameField.delegate = self
        nameField.addTarget(self, action: #selector(textFieldDidChange(_:)), for: .editingChanged)
        nameField.translatesAutoresizingMaskIntoConstraints = false
        contentView.addSubview(nameField)

        slugPreview.font = .monospacedSystemFont(ofSize: 11, weight: .regular)
        slugPreview.textColor = .tertiaryLabelColor
        slugPreview.translatesAutoresizingMaskIntoConstraints = false
        contentView.addSubview(slugPreview)
    }

    private func setupPromptTextView() {
        promptTextView.isEditable = true
        promptTextView.font = .systemFont(ofSize: 16)
        promptTextView.textColor = .labelColor
        promptTextView.backgroundColor = .textBackgroundColor
        promptTextView.textContainer.widthTracksTextView = true
        promptTextView.textContainer.lineFragmentPadding = 8
        promptTextView.textContainerInset = UIEdgeInsets(top: 6, left: 0, bottom: 6, right: 0)
        promptTextView.delegate = self
        promptTextView.smartQuotesType = .no
        promptTextView.smartDashesType = .no
        promptTextView.smartInsertDeleteType = .no
        promptTextView.autocorrectionType = .no
        promptTextView.autocapitalizationType = .none
        promptTextView.alwaysBounceHorizontal = false
        promptTextView.layer.borderWidth = 1
        promptTextView.translatesAutoresizingMaskIntoConstraints = false
        promptTextView.onSubmit = { [weak self] in self?.submitPrompt() }
        contentView.addSubview(promptTextView)
        updatePromptBorderColor()
        registerForTraitChanges([UITraitUserInterfaceStyle.self]) { (self: Self, _) in
            self.updatePromptBorderColor()
        }

        promptPlaceholder.font = promptTextView.font
        promptPlaceholder.textColor = .placeholderTextColor
        promptPlaceholder.translatesAutoresizingMaskIntoConstraints = false
        promptPlaceholder.isUserInteractionEnabled = false
        promptTextView.addSubview(promptPlaceholder)
        NSLayoutConstraint.activate([
            promptPlaceholder.topAnchor.constraint(equalTo: promptTextView.topAnchor, constant: 6),
            promptPlaceholder.leadingAnchor.constraint(equalTo: promptTextView.leadingAnchor, constant: 8),
            promptPlaceholder.widthAnchor.constraint(lessThanOrEqualTo: promptTextView.widthAnchor, constant: -16),
        ])

        promptHint.text = "Enter to submit \u{00B7} Shift+Enter for newline"
        promptHint.font = .systemFont(ofSize: 12)
        promptHint.textColor = .secondaryLabelColor
        promptHint.textAlignment = .center
        promptHint.translatesAutoresizingMaskIntoConstraints = false
        contentView.addSubview(promptHint)
    }

    /// The bezel border AppKit draws around the prompt's scroll view.
    private func updatePromptBorderColor() {
        promptTextView.layer.borderColor = NSColor.separatorColor.resolvedColor(with: traitCollection).cgColor
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

        searchField.text = ""
        searchField.isHidden = false
        separatorView.isHidden = false
        tableView.isHidden = false

        promptHeader.isHidden = true
        promptTextView.isHidden = true
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
            self?.focusSearchFieldIfSelecting()
        }
    }

    /// Deferred search focus must not steal focus from a naming/prompt step entered
    /// in the meantime (e.g. `show(initialItem:)` selects an item right after loading).
    private func focusSearchFieldIfSelecting() {
        guard case .selection = phase, view.window != nil else { return }
        searchField.becomeFirstResponder()
    }

    /// Lays out the naming step directly (never via the prompt layout) so the panel
    /// resizes once and focus lands in the name field before any typed keys are delivered.
    private func showNamingPhase(item: CommandPaletteItem) {
        phase = .naming(item)
        prepareItemPhase(item: item)
        promptHeaderLabel.text = "Name \(item.displayName)"

        nameLabel.isHidden = false
        nameField.isHidden = false
        nameField.placeholder = "e.g., Fix authentication"
        slugPreview.isHidden = false
        slugPreview.text = "Enter to continue"
        promptScrollTopConstraint?.isActive = false
        NSLayoutConstraint.activate(nameFieldConstraints)
        promptTextView.isHidden = true
        promptHint.isHidden = true

        focus(nameField)
        resizePanelHeight(to: 150)
    }

    private func submitName() {
        guard case .naming(let item) = phase else { return }
        let name = (nameField.text ?? "").trimmingCharacters(in: .whitespacesAndNewlines)
        guard !name.isEmpty else {
            slugPreview.text = "Enter a name to continue"
            return
        }
        nameField.text = name
        if item.skipsPromptPhase {
            if case .agentDef(let def) = item {
                onSelect?(.spawnAgentDef(def: def, prompt: nil, name: name))
            }
        } else {
            showPromptPhase(item: item)
        }
    }

    /// Shared setup for the naming and prompt steps: hide selection UI, show the item header,
    /// and reset drafts when switching to a different item.
    private func prepareItemPhase(item: CommandPaletteItem) {
        searchField.isHidden = true
        separatorView.isHidden = true
        tableView.isHidden = true

        let img = NSImage(systemSymbolName: item.icon, accessibilityDescription: item.displayName)
        promptHeaderIcon.image = img
        promptHeaderIcon.tintColor = .labelColor
        promptHeaderLabel.text = item.displayName
        promptHeader.isHidden = false

        if draftItemID != item.id {
            promptTextView.text = ""
            nameField.text = ""
            draftItemID = item.id
        }
        promptPlaceholder.text = item.promptPlaceholder
        updatePromptPlaceholder()
    }

    /// Moves focus synchronously when the panel is on screen, so keystrokes queued behind the
    /// Enter that triggered a transition go to the new field rather than the previous one.
    private func focus(_ responder: UIResponder) {
        if view.window != nil {
            responder.becomeFirstResponder()
        } else {
            DispatchQueue.main.async {
                responder.becomeFirstResponder()
            }
        }
    }

    private func showPromptPhase(item: CommandPaletteItem) {
        phase = .prompt(item)
        prepareItemPhase(item: item)

        let showName = item.showsNameField
        nameLabel.isHidden = !showName
        nameField.isHidden = !showName
        slugPreview.isHidden = !showName
        nameField.placeholder = "e.g., fix-auth-bug"
        slugPreview.text = ""

        // Swap the dynamic top constraint for the prompt
        promptScrollTopConstraint?.isActive = false
        if showName {
            NSLayoutConstraint.activate(nameFieldConstraints)
            promptScrollTopConstraint = promptTextView.topAnchor.constraint(
                equalTo: slugPreview.bottomAnchor, constant: 12)
        } else {
            NSLayoutConstraint.deactivate(nameFieldConstraints)
            promptScrollTopConstraint = promptTextView.topAnchor.constraint(
                equalTo: promptHeader.bottomAnchor, constant: 16)
        }
        promptScrollTopConstraint?.isActive = true

        let showPrompt = item.showsPromptField

        promptTextView.isHidden = !showPrompt
        promptHint.isHidden = !showPrompt

        promptHeightConstraint.constant = 60

        let panelHeight: CGFloat
        if showName && showPrompt {
            panelHeight = 260
        } else if showName {
            // Name only (worktree) — no prompt field
            panelHeight = 140
        } else {
            panelHeight = 170
        }
        focus(showName ? nameField : promptTextView)
        resizePanelHeight(to: panelHeight)
    }

    private func resizePanelHeight(to height: CGFloat, animate: Bool = true) {
        (parent as? CommandPalettePanel)?.setPanelHeight(height, animate: animate)
    }

    private func resizePanel(rowCount: Int) {
        resizePanelHeight(to: Self.selectionPanelHeight(rowCount: rowCount))
    }

    /// The search header, one 44pt row per result (at least one), and the bottom padding,
    /// capped at the panel's full height.
    static func selectionPanelHeight(rowCount: Int) -> CGFloat {
        let headerHeight: CGFloat = 57
        let rowsHeight = CGFloat(max(rowCount, 1)) * 44
        let bottomPadding: CGFloat = 8
        return min(headerHeight + rowsHeight + bottomPadding, CommandPalettePanel.maxPanelHeight)
    }

    // MARK: - Filtering

    private func filterItems(query: String) {
        filteredItems = Self.filter(availableItems, query: query)
        selectedIndex = filteredItems.isEmpty ? -1 : 0
        tableView.reloadData()
        updateHighlight()
        resizePanel(rowCount: filteredItems.count)
    }

    /// The Mac palette's ranking: name prefix, then name substring, then any searchable text,
    /// then (files only) an in-order subsequence; files are capped.
    static func filter(_ items: [CommandPaletteItem], query: String) -> [CommandPaletteItem] {
        let q = query.lowercased().trimmingCharacters(in: .whitespaces)
        if q.isEmpty {
            return items.filter { !$0.isQueryOnly }
        }
        let scored =
            items
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
                } else if item.isQueryOnly, isSubsequence(q, of: text) {
                    score = 5
                } else {
                    return nil
                }
                return (item, score)
            }
            .sorted { $0.1 > $1.1 }
            .map(\.0)
        // File items can number in the thousands; keep the list scannable.
        var fileCount = 0
        return scored.filter { item in
            guard item.isQueryOnly else { return true }
            fileCount += 1
            return fileCount <= maxFileResults
        }
    }

    private static let maxFileResults = 30

    /// `q` appears in `text` in order, not necessarily contiguously ("edst" matches "EditorState").
    private static func isSubsequence(_ q: String, of text: String) -> Bool {
        var it = text.makeIterator()
        return q.allSatisfy { ch in
            while let next = it.next() { if next == ch { return true } }
            return false
        }
    }

    // MARK: - Highlight

    private func updateHighlight() {
        for indexPath in tableView.indexPathsForVisibleRows ?? [] {
            guard let cellView = tableView.cellForRow(at: indexPath) as? CommandPaletteRowView else { continue }
            cellView.setHighlighted(indexPath.row == selectedIndex)
        }
    }

    private func scrollSelectedRowToVisible() {
        guard selectedIndex >= 0, selectedIndex < filteredItems.count else { return }
        tableView.scrollToRow(at: IndexPath(row: selectedIndex, section: 0), at: .none, animated: false)
    }

    // MARK: - Actions

    private func selectCurrentItem() {
        guard selectedIndex >= 0, selectedIndex < filteredItems.count else { return }
        let item = filteredItems[selectedIndex]
        selectItem(item)
    }

    func selectItem(_ item: CommandPaletteItem) {
        if requiresName(item) {
            showNamingPhase(item: item)
        } else if item.skipsPromptPhase {
            let result: CommandPaletteResult
            switch item {
            case .agentDef(let d): result = .spawnAgentDef(def: d, prompt: nil, name: nil)
            case .swarm(let s): result = .runSwarm(def: s)
            case .files: result = .openFilePane(path: nil)
            case .file(let f): result = .openFilePane(path: f.absolutePath)
            case .builtIn: return
            }
            onSelect?(result)
        } else {
            showPromptPhase(item: item)
        }
    }

    private func submitPrompt() {
        guard case .prompt(let item) = phase else { return }
        let prompt = promptTextView.text.trimmingCharacters(in: .whitespacesAndNewlines)
        let name = (item.showsNameField || requiresName(item))
            ? (nameField.text ?? "").trimmingCharacters(in: .whitespacesAndNewlines) : nil
        guard !requiresName(item) || name?.isEmpty == false else { return }
        let promptOrNil = prompt.isEmpty ? nil : prompt
        let nameOrNil = name?.isEmpty == true ? nil : name

        let result: CommandPaletteResult
        switch item {
        case .builtIn(let v):
            if v.kind == .worktree {
                result = .createWorktree(name: nameOrNil)
            } else {
                result = .spawnBuiltIn(variant: v, prompt: promptOrNil, name: nameOrNil)
            }
        case .agentDef(let d): result = .spawnAgentDef(def: d, prompt: promptOrNil, name: nameOrNil)
        case .swarm(let s): result = .runSwarm(def: s)
        case .files: result = .openFilePane(path: nil)
        case .file(let f): result = .openFilePane(path: f.absolutePath)
        }
        onSelect?(result)
    }

    func handleEscape() {
        switch phase {
        case .selection:
            onDismiss?()
        case .naming:
            showSelectionPhase()
        case .prompt(let item):
            if requiresName(item) { showNamingPhase(item: item) } else { showSelectionPhase() }
        }
    }

    private func row(at recognizer: UIGestureRecognizer) -> Int? {
        let point = recognizer.location(in: tableView)
        guard let indexPath = tableView.indexPathForRow(at: point), indexPath.row < filteredItems.count else {
            return nil
        }
        return indexPath.row
    }

    @objc private func rowTapped(_ recognizer: UITapGestureRecognizer) {
        guard let row = row(at: recognizer) else { return }
        selectedIndex = row
        updateHighlight()
        selectCurrentItem()
    }

    @objc private func rowClicked(_ recognizer: UITapGestureRecognizer) {
        guard let row = row(at: recognizer) else { return }
        selectedIndex = row
        updateHighlight()
    }

    @objc private func rowDoubleClicked(_ recognizer: UITapGestureRecognizer) {
        guard let row = row(at: recognizer) else { return }
        selectedIndex = row
        updateHighlight()
        selectCurrentItem()
    }

    // MARK: - Keys

    override var keyCommands: [UIKeyCommand]? {
        var commands = [Self.command(UIKeyCommand.inputEscape, #selector(escapePressed))]
        if case .selection = phase, searchField.isFirstResponder {
            commands.append(Self.command(UIKeyCommand.inputUpArrow, #selector(moveUpPressed)))
            commands.append(Self.command(UIKeyCommand.inputDownArrow, #selector(moveDownPressed)))
        }
        if nameField.isFirstResponder {
            commands.append(Self.command("\t", #selector(nameFieldTabPressed)))
        }
        return commands
    }

    /// The palette's keys come before the text field's own handling of them.
    private static func command(_ input: String, _ action: Selector) -> UIKeyCommand {
        let command = UIKeyCommand(input: input, modifierFlags: [], action: action)
        command.wantsPriorityOverSystemBehavior = true
        return command
    }

    @objc private func escapePressed() {
        handleEscape()
    }

    @objc private func moveUpPressed() {
        if selectedIndex > 0 {
            selectedIndex -= 1
            updateHighlight()
            scrollSelectedRowToVisible()
        }
    }

    @objc private func moveDownPressed() {
        if selectedIndex < filteredItems.count - 1 {
            selectedIndex += 1
            updateHighlight()
            scrollSelectedRowToVisible()
        }
    }

    @objc private func nameFieldTabPressed() {
        if case .prompt(let item) = phase, item.showsPromptField {
            promptTextView.becomeFirstResponder()
        }
    }

    // MARK: - UITextFieldDelegate

    @objc private func textFieldDidChange(_ field: UITextField) {
        if field === searchField {
            filterItems(query: field.text ?? "")
        } else if field === nameField {
            if case .naming = phase {
                slugPreview.text = "Enter to continue"
            } else {
                updateSlugPreview()
            }
        }
    }

    func textFieldShouldReturn(_ textField: UITextField) -> Bool {
        if textField === searchField {
            selectCurrentItem()
        } else if textField === nameField {
            if case .naming = phase {
                submitName()
            } else if case .prompt(let item) = phase, !item.showsPromptField {
                submitPrompt()
            } else {
                promptTextView.becomeFirstResponder()
            }
        }
        return false
    }

    // MARK: - UITextViewDelegate

    func textView(_ textView: UITextView, shouldChangeTextIn range: NSRange, replacementText text: String) -> Bool {
        // The on-screen keyboard's return submits, like Enter; Shift+Enter arrives as a newline
        // the text view inserts itself.
        guard textView === promptTextView, text == "\n", !promptTextView.isInsertingNewline else { return true }
        submitPrompt()
        return false
    }

    func textViewDidChange(_ textView: UITextView) {
        guard textView === promptTextView else { return }
        updatePromptPlaceholder()
        adjustPromptHeight()
    }

    private func updatePromptPlaceholder() {
        promptPlaceholder.isHidden = !promptTextView.text.isEmpty
    }

    private func updateSlugPreview() {
        let raw = nameField.text ?? ""
        let slug = WorktreeNameNormalizer.normalize(raw)
        slugPreview.text = slug.isEmpty ? "" : "pu/\(slug)"
    }

    private func adjustPromptHeight() {
        let width = promptTextView.bounds.width
        guard width > 0 else { return }
        // `sizeThatFits` includes the vertical insets the Mac adds to the used rect.
        let fitted = promptTextView.sizeThatFits(CGSize(width: width, height: .greatestFiniteMagnitude)).height
        let newHeight = min(max(fitted + 4, 60), 120)
        promptHeightConstraint.constant = newHeight

        let panelHeight: CGFloat = 16 + 22 + 16 + newHeight + 12 + 16 + 16
        resizePanelHeight(to: panelHeight, animate: false)
    }
}

// MARK: - UITableViewDataSource & Delegate

extension CommandPaletteViewController: UITableViewDataSource, UITableViewDelegate {

    func tableView(_ tableView: UITableView, numberOfRowsInSection section: Int) -> Int {
        filteredItems.count
    }

    func tableView(_ tableView: UITableView, cellForRowAt indexPath: IndexPath) -> UITableViewCell {
        let item = filteredItems[indexPath.row]
        let cellView =
            tableView.dequeueReusableCell(
                withIdentifier: CommandPaletteRowView.identifier, for: indexPath
            ) as? CommandPaletteRowView
            ?? CommandPaletteRowView()
        cellView.configure(with: item, isHighlighted: indexPath.row == selectedIndex)
        return cellView
    }
}

// MARK: - UIGestureRecognizerDelegate

extension CommandPaletteViewController: UIGestureRecognizerDelegate {
    /// A double-click's first click highlights its row, as the Mac's does.
    func gestureRecognizer(
        _ gestureRecognizer: UIGestureRecognizer,
        shouldRecognizeSimultaneouslyWith otherGestureRecognizer: UIGestureRecognizer
    ) -> Bool {
        true
    }
}

// MARK: - CommandPalettePromptTextView

/// The prompt field. A hardware Enter submits and Shift+Enter or Option+Enter inserts a
/// newline, as the Mac prompt's `insertNewline` handling does.
final class CommandPalettePromptTextView: UITextView {
    var onSubmit: (() -> Void)?
    /// Set while the view inserts a newline itself, so the delegate lets it through.
    private(set) var isInsertingNewline = false

    override func pressesBegan(_ presses: Set<UIPress>, with event: UIPressesEvent?) {
        guard markedTextRange == nil,
            let key = presses.first(where: { $0.key != nil })?.key,
            key.keyCode == .keyboardReturnOrEnter || key.keyCode == .keypadEnter
        else {
            super.pressesBegan(presses, with: event)
            return
        }
        if key.modifierFlags.contains(.shift) || key.modifierFlags.contains(.alternate) {
            isInsertingNewline = true
            insertText("\n")
            isInsertingNewline = false
        } else {
            onSubmit?()
        }
    }
}
#endif
