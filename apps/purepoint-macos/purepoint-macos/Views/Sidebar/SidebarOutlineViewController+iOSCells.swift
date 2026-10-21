#if os(iOS)
import UIKit

// MARK: - Row cell

/// One sidebar row, laid out like a row of the Mac's source-list `NSOutlineView`: inset from
/// the sidebar edges, a disclosure chevron in the indentation, and the row's own content in a
/// horizontal stack built by the controller's row factories.
final class SidebarRowCell: UICollectionViewCell {
    /// The Mac outline's `indentationPerLevel`.
    static let indentationPerLevel: CGFloat = 12
    /// The source-list style insets each row this far from both sidebar edges.
    static let rowInset: CGFloat = 10
    /// The disclosure column in front of a row's content.
    static let disclosureWidth: CGFloat = 16

    /// The row's frame within the cell: what the selection fills and context menus lift.
    let rowView = UIView()
    let disclosureButton = UIButton(type: .custom)
    /// The Mac cell view: the row's content, placed after the indentation.
    let cellView = UIView()
    private(set) var stack = UIStackView()
    private var indentConstraint: NSLayoutConstraint!
    private let toolTipInteraction = UIToolTipInteraction()

    /// The workspace row's name, the label inline rename edits.
    weak var nameLabel: UILabel?
    private(set) weak var renameField: SidebarRenameField?
    var isRenaming: Bool { renameField != nil }

    /// Rebuilds the content for the current selection emphasis.
    var fillContent: ((SidebarRowCell) -> Void)?
    var onToggleExpansion: (() -> Void)?

    /// Whether the outline has keyboard focus, so a selected row shows the accent selection.
    var isEmphasized = false {
        didSet { if oldValue != isEmphasized { refresh() } }
    }

    /// Selected and focused: the Mac draws this row with the accent fill and white content.
    var showsEmphasizedSelection: Bool { isSelected && isEmphasized }

    override var isSelected: Bool {
        didSet { if oldValue != isSelected { refresh() } }
    }

    override var isHighlighted: Bool {
        didSet { updateSelectionBackground() }
    }

    var toolTip: String? {
        get { toolTipInteraction.defaultToolTip }
        set { toolTipInteraction.defaultToolTip = newValue }
    }

    override init(frame: CGRect) {
        super.init(frame: frame)
        rowView.translatesAutoresizingMaskIntoConstraints = false
        rowView.layer.cornerRadius = 5
        rowView.layer.cornerCurve = .continuous
        contentView.addSubview(rowView)

        var chevron = UIButton.Configuration.plain()
        chevron.contentInsets = .zero
        chevron.image = UIImage(
            systemName: "chevron.right",
            withConfiguration: UIImage.SymbolConfiguration(pointSize: 9, weight: .bold))
        disclosureButton.configuration = chevron
        disclosureButton.tintColor = .tertiaryLabelColor
        disclosureButton.isPointerInteractionEnabled = true
        disclosureButton.translatesAutoresizingMaskIntoConstraints = false
        disclosureButton.addAction(UIAction { [weak self] _ in self?.onToggleExpansion?() }, for: .primaryActionTriggered)
        disclosureButton.accessibilityLabel = "Disclosure"
        rowView.addSubview(disclosureButton)

        cellView.translatesAutoresizingMaskIntoConstraints = false
        rowView.addSubview(cellView)
        cellView.addInteraction(toolTipInteraction)

        indentConstraint = disclosureButton.leadingAnchor.constraint(equalTo: rowView.leadingAnchor)
        NSLayoutConstraint.activate([
            rowView.topAnchor.constraint(equalTo: contentView.topAnchor),
            rowView.bottomAnchor.constraint(equalTo: contentView.bottomAnchor),
            rowView.leadingAnchor.constraint(equalTo: contentView.leadingAnchor, constant: Self.rowInset),
            rowView.trailingAnchor.constraint(equalTo: contentView.trailingAnchor, constant: -Self.rowInset),
            indentConstraint,
            disclosureButton.widthAnchor.constraint(equalToConstant: Self.disclosureWidth - 2),
            disclosureButton.topAnchor.constraint(equalTo: rowView.topAnchor),
            disclosureButton.bottomAnchor.constraint(equalTo: rowView.bottomAnchor),
            cellView.leadingAnchor.constraint(equalTo: disclosureButton.leadingAnchor, constant: Self.disclosureWidth),
            cellView.trailingAnchor.constraint(equalTo: rowView.trailingAnchor),
            cellView.topAnchor.constraint(equalTo: rowView.topAnchor),
            cellView.bottomAnchor.constraint(equalTo: rowView.bottomAnchor),
        ])
        isAccessibilityElement = true
    }

    required init?(coder: NSCoder) {
        fatalError("init(coder:) is not supported: sidebar rows are built in code")
    }

    override func prepareForReuse() {
        super.prepareForReuse()
        renameField?.removeFromSuperview()
        fillContent = nil
        onToggleExpansion = nil
        toolTip = nil
        accessibilityLabel = nil
        accessibilityCustomActions = nil
    }

    /// Places the row at its outline level and shows its disclosure state.
    func configure(level: Int, isExpandable: Bool, isExpanded: Bool) {
        indentConstraint.constant = CGFloat(level) * Self.indentationPerLevel
        disclosureButton.isHidden = !isExpandable
        disclosureButton.transform = isExpanded ? CGAffineTransform(rotationAngle: .pi / 2) : .identity
        if isExpandable {
            accessibilityValue = isExpanded ? "Expanded" : "Collapsed"
        } else {
            accessibilityValue = nil
        }
        refresh()
    }

    /// A fresh content stack, as the Mac builds a new cell view for each row it shows.
    func resetStack(spacing: CGFloat) -> UIStackView {
        stack.removeFromSuperview()
        let stack = UIStackView()
        stack.axis = .horizontal
        stack.alignment = .center
        stack.spacing = spacing
        stack.translatesAutoresizingMaskIntoConstraints = false
        cellView.addSubview(stack)
        NSLayoutConstraint.activate([
            stack.leadingAnchor.constraint(equalTo: cellView.leadingAnchor, constant: 2),
            stack.trailingAnchor.constraint(equalTo: cellView.trailingAnchor, constant: -4),
            stack.centerYAnchor.constraint(equalTo: cellView.centerYAnchor),
        ])
        self.stack = stack
        nameLabel = nil
        return stack
    }

    func refresh() {
        updateSelectionBackground()
        disclosureButton.tintColor = showsEmphasizedSelection ? .white : .tertiaryLabelColor
        // A row being renamed keeps its field; rebuilding would drop the edit.
        guard !isRenaming else { return }
        fillContent?(self)
    }

    private func updateSelectionBackground() {
        if isSelected {
            rowView.backgroundColor = isEmphasized ? .controlAccentColor : .unemphasizedSelectedContentBackgroundColor
        } else if isHighlighted {
            rowView.backgroundColor = .unemphasizedSelectedContentBackgroundColor
        } else {
            rowView.backgroundColor = .clear
        }
    }

    // MARK: Inline rename

    /// Swaps the name label for an editable field holding the same text, in the same place.
    func beginRenaming() -> SidebarRenameField? {
        guard let label = nameLabel, let index = stack.arrangedSubviews.firstIndex(of: label) else { return nil }
        let field = SidebarRenameField()
        field.font = label.font
        field.text = label.text
        field.textColor = .labelColor
        field.borderStyle = .none
        field.returnKeyType = .done
        field.autocorrectionType = .no
        field.autocapitalizationType = .none
        field.spellCheckingType = .no
        field.setContentHuggingPriority(.defaultLow, for: .horizontal)
        field.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        label.isHidden = true
        stack.insertArrangedSubview(field, at: index + 1)
        renameField = field
        return field
    }

    /// Puts the label back showing `text`.
    func endRenaming(showing text: String?) {
        nameLabel?.text = text
        nameLabel?.isHidden = false
        renameField?.removeFromSuperview()
        // The label keeps the typed name until the rename reaches the state and the row rebuilds.
        updateSelectionBackground()
    }
}

/// The inline rename field: Escape cancels, as in the Mac field editor.
final class SidebarRenameField: UITextField {
    var onCancel: (() -> Void)?

    override var keyCommands: [UIKeyCommand]? {
        let escape = UIKeyCommand(input: UIKeyCommand.inputEscape, modifierFlags: [], action: #selector(cancelRename))
        escape.wantsPriorityOverSystemBehavior = true
        return [escape]
    }

    @objc private func cancelRename() {
        onCancel?()
    }
}

// MARK: - Row factories

/// The Mac cell factories, building into a `SidebarRowCell`'s stack with the same views,
/// fonts, colors and spacing.
@MainActor
struct SidebarRowContent {
    /// Under the accent selection, content the Mac colors through the cell's background style
    /// turns white.
    let emphasized: Bool

    func tone(_ color: UIColor) -> UIColor { emphasized ? .white : color }

    func label(_ text: String, font: UIFont, color: UIColor = .labelColor) -> UILabel {
        let label = UILabel()
        label.text = text
        label.font = font
        label.textColor = tone(color)
        label.lineBreakMode = .byTruncatingTail
        return label
    }

    func symbolIcon(_ name: String, description: String, pointSize: CGFloat) -> UIImageView {
        let icon = UIImageView(
            image: UIImage(
                systemName: name, withConfiguration: UIImage.SymbolConfiguration(pointSize: pointSize, weight: .regular)))
        icon.tintColor = tone(.secondaryLabelColor)
        icon.contentMode = .center
        icon.accessibilityLabel = description
        icon.setContentHuggingPriority(.required, for: .horizontal)
        icon.setContentCompressionResistancePriority(.required, for: .horizontal)
        return icon
    }

    func statusDot(color: UIColor) -> UIView {
        let dot = UIView()
        dot.backgroundColor = color
        dot.layer.cornerRadius = PurePointTheme.statusDotSize / 2
        dot.translatesAutoresizingMaskIntoConstraints = false
        dot.widthAnchor.constraint(equalToConstant: PurePointTheme.statusDotSize).isActive = true
        dot.heightAnchor.constraint(equalToConstant: PurePointTheme.statusDotSize).isActive = true
        dot.setContentHuggingPriority(.required, for: .horizontal)
        return dot
    }

    func badge(count: Int) -> UILabel {
        let badge = SidebarPaddedLabel()
        badge.text = "\(count)"
        badge.font = .systemFont(ofSize: 10)
        badge.textColor = tone(.secondaryLabelColor)
        badge.textAlignment = .center
        badge.backgroundColor = .quaternaryLabelColor
        badge.layer.cornerRadius = 6
        badge.clipsToBounds = true
        badge.setContentHuggingPriority(.required, for: .horizontal)
        badge.setContentCompressionResistancePriority(.required, for: .horizontal)
        let width = max(18, badge.intrinsicContentSize.width + 8)
        badge.translatesAutoresizingMaskIntoConstraints = false
        badge.widthAnchor.constraint(equalToConstant: width).isActive = true
        return badge
    }

    /// The signal pill after a workspace name.
    func pill(_ signal: AgentSignalPresentation) -> UILabel {
        let pill = SidebarPaddedLabel()
        pill.text = " \(signal.label) "
        pill.font = .systemFont(ofSize: 9, weight: .medium)
        pill.textColor = tone(signal.needsAttention ? .labelColor : .secondaryLabelColor)
        pill.backgroundColor = .quaternaryLabelColor
        pill.layer.cornerRadius = 4
        pill.clipsToBounds = true
        pill.setContentCompressionResistancePriority(.required, for: .horizontal)
        pill.setContentHuggingPriority(.required, for: .horizontal)
        return pill
    }

    func countIcon(_ symbol: String, description: String, count: Int) -> UIStackView {
        let icon = UIImageView(
            image: UIImage(systemName: symbol, withConfiguration: UIImage.SymbolConfiguration(pointSize: 9, weight: .regular)))
        icon.tintColor = tone(.tertiaryLabelColor)
        icon.contentMode = .center
        icon.setContentHuggingPriority(.required, for: .horizontal)

        let label = UILabel()
        label.text = "\(count)"
        label.font = .systemFont(ofSize: 10)
        label.textColor = tone(.tertiaryLabelColor)
        label.setContentHuggingPriority(.required, for: .horizontal)

        let stack = UIStackView(arrangedSubviews: [icon, label])
        stack.axis = .horizontal
        stack.alignment = .center
        stack.spacing = 2
        stack.accessibilityLabel = "\(count) \(description)"
        stack.setContentHuggingPriority(.required, for: .horizontal)
        stack.setContentCompressionResistancePriority(.required, for: .horizontal)
        return stack
    }

    func spacer() -> UIView {
        let view = UIView()
        view.setContentHuggingPriority(.defaultLow, for: .horizontal)
        view.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        return view
    }

    /// The row's `+` button: an action, or a menu shown on press.
    func inlineAddButton(action: UIAction? = nil, menu: UIMenu? = nil) -> UIButton {
        var config = UIButton.Configuration.plain()
        config.contentInsets = .zero
        config.image = UIImage(systemName: "plus.circle")
        config.preferredSymbolConfigurationForImage = UIImage.SymbolConfiguration(pointSize: 13, weight: .regular)
        let button = UIButton(configuration: config)
        button.tintColor = tone(.secondaryLabelColor)
        button.accessibilityLabel = "Add"
        button.isPointerInteractionEnabled = true
        if let action { button.addAction(action, for: .primaryActionTriggered) }
        if let menu {
            button.menu = menu
            button.showsMenuAsPrimaryAction = true
        }
        button.setContentHuggingPriority(.required, for: .horizontal)
        button.translatesAutoresizingMaskIntoConstraints = false
        button.widthAnchor.constraint(equalToConstant: 16).isActive = true
        button.heightAnchor.constraint(equalToConstant: 16).isActive = true
        return button
    }
}

/// A label whose intrinsic width matches AppKit's label, which pads its text by 2pt a side.
final class SidebarPaddedLabel: UILabel {
    override var intrinsicContentSize: CGSize {
        let size = super.intrinsicContentSize
        return CGSize(width: size.width + 4, height: size.height)
    }
}
#endif
