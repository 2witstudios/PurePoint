#if os(iOS)
import UIKit

/// Keyboard movement through the visible file rows, as `NSOutlineView` moves its selection:
/// up and down step a row, right opens a closed folder, and left closes an open folder or
/// moves to the row's parent.
enum FileTreeKeyNavigation {
    enum Key: Equatable {
        case up, down, left, right
    }

    struct Row: Equatable {
        /// Index of the parent row among the visible rows.
        var parent: Int?
        var isExpandable = false
        var isExpanded = false
    }

    enum Command: Equatable {
        case select(Int)
        case expand(Int)
        case collapse(Int)
        case none
    }

    static func command(for key: Key, rows: [Row], selected: Int?) -> Command {
        let selected = selected.flatMap { rows.indices.contains($0) ? $0 : nil }
        switch key {
        case .down:
            let next = selected.map { $0 + 1 } ?? 0
            return rows.indices.contains(next) ? .select(next) : .none
        case .up:
            let previous = (selected ?? rows.count) - 1
            return rows.indices.contains(previous) ? .select(previous) : .none
        case .left:
            guard let selected else { return .none }
            let row = rows[selected]
            if row.isExpandable, row.isExpanded { return .collapse(selected) }
            return row.parent.map(Command.select) ?? .none
        case .right:
            guard let selected else { return .none }
            let row = rows[selected]
            return row.isExpandable && !row.isExpanded ? .expand(selected) : .none
        }
    }
}

/// `NSTableView` type-select: typed characters build a prefix (reset after a pause), and the
/// selection moves to the first visible row from the selected one whose name starts with it.
struct FileTreeTypeSelect {
    /// How long a pause ends the typed prefix, as AppKit's key-repeat-based timeout.
    static let timeout: TimeInterval = 1

    private(set) var prefix = ""
    private var lastKey: Date?

    /// The row to select after typing `characters` at `now`, or nil when no row matches.
    mutating func row(typing characters: String, names: [String], selected: Int?, now: Date = Date()) -> Int? {
        let continuing = lastKey.map { now.timeIntervalSince($0) < Self.timeout } ?? false
        lastKey = now
        prefix = continuing ? prefix + characters : characters
        guard !names.isEmpty else { return nil }
        // A new prefix searches past the selected row; a longer one may still match it.
        let start = selected.map { continuing ? $0 : $0 + 1 } ?? 0
        let order = (0..<names.count).map { (start + $0) % names.count }
        return order.first { names[$0].lowercased().hasPrefix(prefix.lowercased()) }
    }
}

/// The file tree's collection view: the iOS twin of the Mac's `NSOutlineView`. It takes the
/// keyboard itself when first responder, so arrow keys and type-select move the selection
/// rather than the focus system.
final class FileTreeKeyOutlineView: UICollectionView {
    /// An arrow key pressed; true if handled.
    var onArrowKey: ((FileTreeKeyNavigation.Key) -> Bool)?
    /// Printable characters typed without a command modifier; true if handled.
    var onTypeSelect: ((String) -> Bool)?
    /// Called when the outline gains or loses keyboard focus, which changes the selection color.
    var onFirstResponderChanged: (() -> Void)?

    override var canBecomeFirstResponder: Bool { true }

    @discardableResult
    override func becomeFirstResponder() -> Bool {
        let became = super.becomeFirstResponder()
        if became { onFirstResponderChanged?() }
        return became
    }

    @discardableResult
    override func resignFirstResponder() -> Bool {
        let resigned = super.resignFirstResponder()
        if resigned { onFirstResponderChanged?() }
        return resigned
    }

    override func pressesBegan(_ presses: Set<UIPress>, with event: UIPressesEvent?) {
        var unhandled: Set<UIPress> = []
        for press in presses {
            if let key = press.key, handle(key) { continue }
            unhandled.insert(press)
        }
        if !unhandled.isEmpty {
            super.pressesBegan(unhandled, with: event)
        }
    }

    /// Plain keys only: a modified key belongs to the app's key commands.
    func handle(_ key: UIKey) -> Bool {
        guard key.modifierFlags.intersection([.command, .control, .alternate]).isEmpty else { return false }
        switch key.keyCode {
        case .keyboardUpArrow: return onArrowKey?(.up) == true
        case .keyboardDownArrow: return onArrowKey?(.down) == true
        case .keyboardLeftArrow: return onArrowKey?(.left) == true
        case .keyboardRightArrow: return onArrowKey?(.right) == true
        default:
            let characters = key.characters
            guard Self.isTypeSelectInput(characters) else { return false }
            return onTypeSelect?(characters) == true
        }
    }

    /// Type-select takes printable text; whitespace, controls and function keys pass through.
    nonisolated static func isTypeSelectInput(_ characters: String) -> Bool {
        guard !characters.isEmpty else { return false }
        return characters.unicodeScalars.allSatisfy {
            !CharacterSet.whitespacesAndNewlines.contains($0) && !CharacterSet.controlCharacters.contains($0)
                && !(0xF700...0xF8FF).contains($0.value)
        }
    }
}
#endif
