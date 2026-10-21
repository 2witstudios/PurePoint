#if os(iOS)
import UIKit

/// Placeholder twin of the Mac palette row cell, with the same configuration API.
final class CommandPaletteRowView: UITableViewCell {
    static let identifier = "CommandPaletteRow"

    override init(style: UITableViewCell.CellStyle, reuseIdentifier: String?) {
        super.init(style: style, reuseIdentifier: reuseIdentifier)
        NotYetOnIPad.addLabel("CommandPaletteRowView", to: contentView)
    }

    convenience init() {
        self.init(style: .default, reuseIdentifier: Self.identifier)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError() }

    func configure(with item: CommandPaletteItem, isHighlighted: Bool) {
        setHighlighted(isHighlighted)
    }

    func setHighlighted(_ highlighted: Bool) {
        contentView.backgroundColor = highlighted ? NSColor.controlAccentColor.withAlphaComponent(0.15) : .clear
    }
}
#endif
