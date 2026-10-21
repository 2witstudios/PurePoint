#if os(iOS)
import UIKit

/// One file-tree row, laid out like a row of the Mac's source-list `NSOutlineView`: inset from
/// the pane edges, a disclosure chevron in the indentation, then the Mac cell's icon and name.
final class FileTreeRowCell: UICollectionViewCell {
    /// The Mac outline's `indentationPerLevel`.
    static let indentationPerLevel: CGFloat = 12
    /// The source-list style insets each row this far from both edges.
    static let rowInset: CGFloat = 10
    /// The disclosure column in front of a row's content.
    static let disclosureWidth: CGFloat = 16

    /// The row's frame within the cell: what the selection fills and context menus lift.
    let rowView = UIView()
    let disclosureButton = UIButton(type: .custom)
    let iconView = UIImageView()
    let nameLabel = UILabel()
    private var indentConstraint: NSLayoutConstraint!
    private var iconTint: UIColor = .secondaryLabelColor

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
        didSet { refresh() }
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
        disclosureButton.isPointerInteractionEnabled = true
        disclosureButton.translatesAutoresizingMaskIntoConstraints = false
        disclosureButton.addAction(UIAction { [weak self] _ in self?.onToggleExpansion?() }, for: .primaryActionTriggered)
        disclosureButton.isAccessibilityElement = false
        rowView.addSubview(disclosureButton)

        // The Mac cell: a horizontal stack, spacing 5, inset 2 and 4 from the cell's edges.
        let stack = UIStackView(arrangedSubviews: [iconView, nameLabel])
        stack.axis = .horizontal
        stack.alignment = .center
        stack.spacing = 5
        stack.translatesAutoresizingMaskIntoConstraints = false
        rowView.addSubview(stack)

        iconView.preferredSymbolConfiguration = UIImage.SymbolConfiguration(pointSize: 11, weight: .regular)
        iconView.contentMode = .center
        iconView.setContentHuggingPriority(.required, for: .horizontal)
        iconView.setContentCompressionResistancePriority(.required, for: .horizontal)

        nameLabel.font = .systemFont(ofSize: 12)
        nameLabel.lineBreakMode = .byTruncatingTail
        nameLabel.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)

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
            stack.leadingAnchor.constraint(equalTo: disclosureButton.leadingAnchor, constant: Self.disclosureWidth + 2),
            stack.trailingAnchor.constraint(lessThanOrEqualTo: rowView.trailingAnchor, constant: -4),
            stack.centerYAnchor.constraint(equalTo: rowView.centerYAnchor),
        ])
        isAccessibilityElement = true
    }

    required init?(coder: NSCoder) {
        fatalError("init(coder:) is not supported: file tree rows are built in code")
    }

    override func prepareForReuse() {
        super.prepareForReuse()
        onToggleExpansion = nil
    }

    /// Shows `node` at its outline level with its disclosure state, as the Mac cell factory.
    func configure(node: FileTreeNode, level: Int, isExpanded: Bool) {
        indentConstraint.constant = CGFloat(level) * Self.indentationPerLevel
        disclosureButton.isHidden = !node.isExpandable
        disclosureButton.transform = isExpanded ? CGAffineTransform(rotationAngle: .pi / 2) : .identity

        let symbol = FileTreeRowCell.iconName(for: node)
        iconView.image = UIImage(systemName: symbol) ?? UIImage(systemName: "doc")
        iconTint = node.isDirectory ? .systemBlue : .secondaryLabelColor
        nameLabel.text = node.name

        accessibilityLabel = node.name
        accessibilityHint = node.isDirectory ? "Folder" : "File"
        accessibilityValue = node.isExpandable ? (isExpanded ? "Expanded" : "Collapsed") : nil
        refresh()
    }

    /// The Mac cell's symbol: a filled folder, or the file's language icon.
    static func iconName(for node: FileTreeNode) -> String {
        node.isDirectory ? "folder.fill" : EditorLanguage.detect(from: node.name).icon
    }

    private func refresh() {
        if isSelected {
            rowView.backgroundColor = isEmphasized ? .controlAccentColor : .unemphasizedSelectedContentBackgroundColor
        } else if isHighlighted {
            rowView.backgroundColor = .unemphasizedSelectedContentBackgroundColor
        } else {
            rowView.backgroundColor = .clear
        }
        let emphasized = showsEmphasizedSelection
        disclosureButton.tintColor = emphasized ? .white : .tertiaryLabelColor
        iconView.tintColor = emphasized ? .white : iconTint
        nameLabel.textColor = emphasized ? .white : .labelColor
    }
}
#endif
