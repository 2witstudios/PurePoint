import AppKit

/// Reusable table cell view for command palette rows.
/// Owns its icon, name label, subtitle label, and optional category badge.
final class CommandPaletteRowView: NSTableCellView {

    static let identifier = NSUserInterfaceItemIdentifier("CommandPaletteRow")

    private let iconView = NSImageView()
    private let nameLabel = NSTextField(labelWithString: "")
    private let subtitleLabel = NSTextField(labelWithString: "")
    private let badgeLabel = NSTextField(labelWithString: "")
    private let textStack: NSStackView

    override init(frame frameRect: NSRect) {
        textStack = NSStackView(views: [nameLabel, subtitleLabel, badgeLabel])
        super.init(frame: frameRect)

        wantsLayer = true
        layer?.cornerRadius = 6

        iconView.imageScaling = .scaleProportionallyDown
        iconView.translatesAutoresizingMaskIntoConstraints = false

        nameLabel.font = .boldSystemFont(ofSize: 14)
        nameLabel.textColor = .labelColor
        nameLabel.drawsBackground = false
        nameLabel.isBordered = false

        subtitleLabel.font = .systemFont(ofSize: 12)
        subtitleLabel.textColor = .secondaryLabelColor
        subtitleLabel.drawsBackground = false
        subtitleLabel.isBordered = false

        badgeLabel.font = .systemFont(ofSize: 10, weight: .medium)
        badgeLabel.textColor = .tertiaryLabelColor
        badgeLabel.drawsBackground = false
        badgeLabel.isBordered = false
        badgeLabel.isHidden = true

        textStack.orientation = .horizontal
        textStack.spacing = 8
        textStack.alignment = .firstBaseline
        textStack.translatesAutoresizingMaskIntoConstraints = false

        addSubview(iconView)
        addSubview(textStack)

        NSLayoutConstraint.activate([
            iconView.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 16),
            iconView.centerYAnchor.constraint(equalTo: centerYAnchor),
            iconView.widthAnchor.constraint(equalToConstant: 20),
            iconView.heightAnchor.constraint(equalToConstant: 20),

            textStack.leadingAnchor.constraint(equalTo: iconView.trailingAnchor, constant: 12),
            textStack.centerYAnchor.constraint(equalTo: centerYAnchor),
            textStack.trailingAnchor.constraint(lessThanOrEqualTo: trailingAnchor, constant: -16),
        ])
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError() }

    func configure(with item: CommandPaletteItem, isHighlighted: Bool) {
        iconView.image = NSImage(systemSymbolName: item.icon, accessibilityDescription: item.displayName)
        iconView.contentTintColor = .labelColor
        nameLabel.stringValue = item.displayName
        subtitleLabel.stringValue = item.subtitle

        if let category = item.categoryLabel {
            badgeLabel.stringValue = category
            badgeLabel.isHidden = false
        } else {
            badgeLabel.isHidden = true
        }

        setHighlighted(isHighlighted)
    }

    func setHighlighted(_ highlighted: Bool) {
        layer?.backgroundColor =
            highlighted
            ? NSColor.controlAccentColor.withAlphaComponent(0.15).cgColor
            : NSColor.clear.cgColor
    }
}
