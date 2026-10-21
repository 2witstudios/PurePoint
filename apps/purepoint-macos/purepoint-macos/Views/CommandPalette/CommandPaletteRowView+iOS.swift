#if os(iOS)
import UIKit

/// Reusable table cell for command palette rows: the Mac row's icon, name label, subtitle
/// label and optional category badge, with the same metrics.
final class CommandPaletteRowView: UITableViewCell {

    static let identifier = "CommandPaletteRow"

    private let iconView = UIImageView()
    private let nameLabel = UILabel()
    private let subtitleLabel = UILabel()
    private let badgeLabel = UILabel()
    private let textStack: UIStackView

    override init(style: UITableViewCell.CellStyle, reuseIdentifier: String?) {
        textStack = UIStackView(arrangedSubviews: [nameLabel, subtitleLabel, badgeLabel])
        super.init(style: style, reuseIdentifier: reuseIdentifier)

        backgroundColor = .clear
        selectionStyle = .none
        contentView.layer.cornerRadius = 6

        // AppKit draws a symbol at its natural size (the 13pt system font) and only scales it down.
        iconView.contentMode = .center
        iconView.preferredSymbolConfiguration = UIImage.SymbolConfiguration(pointSize: 13)
        iconView.translatesAutoresizingMaskIntoConstraints = false

        nameLabel.font = .boldSystemFont(ofSize: 14)
        nameLabel.textColor = .labelColor
        nameLabel.setContentCompressionResistancePriority(.defaultHigh + 1, for: .horizontal)

        subtitleLabel.font = .systemFont(ofSize: 12)
        subtitleLabel.textColor = .secondaryLabelColor
        subtitleLabel.lineBreakMode = .byTruncatingTail

        badgeLabel.font = .systemFont(ofSize: 10, weight: .medium)
        badgeLabel.textColor = .tertiaryLabelColor
        badgeLabel.isHidden = true

        textStack.axis = .horizontal
        textStack.spacing = 8
        textStack.alignment = .firstBaseline
        textStack.translatesAutoresizingMaskIntoConstraints = false

        contentView.addSubview(iconView)
        contentView.addSubview(textStack)

        NSLayoutConstraint.activate([
            iconView.leadingAnchor.constraint(equalTo: contentView.leadingAnchor, constant: 16),
            iconView.centerYAnchor.constraint(equalTo: contentView.centerYAnchor),
            iconView.widthAnchor.constraint(equalToConstant: 20),
            iconView.heightAnchor.constraint(equalToConstant: 20),

            textStack.leadingAnchor.constraint(equalTo: iconView.trailingAnchor, constant: 12),
            textStack.centerYAnchor.constraint(equalTo: contentView.centerYAnchor),
            textStack.trailingAnchor.constraint(lessThanOrEqualTo: contentView.trailingAnchor, constant: -16),
        ])
    }

    convenience init() {
        self.init(style: .default, reuseIdentifier: Self.identifier)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError() }

    func configure(with item: CommandPaletteItem, isHighlighted: Bool) {
        iconView.image = NSImage(systemSymbolName: item.icon, accessibilityDescription: item.displayName)
        iconView.tintColor = .labelColor
        nameLabel.text = item.displayName
        subtitleLabel.text = item.subtitle
        accessibilityLabel = item.displayName

        if let category = item.categoryLabel {
            badgeLabel.text = category
            badgeLabel.isHidden = false
        } else {
            badgeLabel.isHidden = true
        }

        setHighlighted(isHighlighted)
    }

    func setHighlighted(_ highlighted: Bool) {
        contentView.backgroundColor =
            highlighted
            ? NSColor.controlAccentColor.withAlphaComponent(0.15)
            : .clear
    }
}
#endif
