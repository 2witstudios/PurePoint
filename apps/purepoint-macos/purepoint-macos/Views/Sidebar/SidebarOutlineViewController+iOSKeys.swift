#if os(iOS)
import UIKit

/// Arrow-key movement through the visible rows, as `NSOutlineView` moves its selection:
/// up and down step over rows that cannot be selected, right opens a closed row, and left
/// closes an open row or moves to the row's parent.
enum SidebarKeyNavigation {
    enum Key: Equatable {
        case up, down, left, right
    }

    struct Row: Equatable {
        /// Index of the parent row among the visible rows.
        var parent: Int?
        var isExpandable = false
        var isExpanded = false
        var isSelectable = true
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
            let start = selected.map { $0 + 1 } ?? 0
            return (start..<rows.count).first { rows[$0].isSelectable }.map(Command.select) ?? .none
        case .up:
            let end = selected ?? rows.count
            return (0..<end).reversed().first { rows[$0].isSelectable }.map(Command.select) ?? .none
        case .left:
            guard let selected else { return .none }
            let row = rows[selected]
            if row.isExpandable, row.isExpanded { return .collapse(selected) }
            if let parent = row.parent, rows[parent].isSelectable { return .select(parent) }
            return .none
        case .right:
            guard let selected else { return .none }
            let row = rows[selected]
            return row.isExpandable && !row.isExpanded ? .expand(selected) : .none
        }
    }
}

/// The sidebar outline, with Return acting on a selected machine row: the iOS twin of the
/// Mac's `NSOutlineView` subclass. It takes the keyboard itself, as the Mac outline does when
/// it is first responder, so arrow keys move the selection rather than the focus system.
final class SidebarKeyOutlineView: UICollectionView {
    /// Return pressed; true if handled.
    var onReturn: (() -> Bool)?
    /// An arrow key pressed; true if handled.
    var onArrowKey: ((SidebarKeyNavigation.Key) -> Bool)?
    /// Called once this view has joined a window, so a focus request made before that can land.
    var onWindowAttached: ((UIView) -> Void)?
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

    override func didMoveToWindow() {
        super.didMoveToWindow()
        guard window != nil else { return }
        onWindowAttached?(self)
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

    /// Plain keys only: a modified arrow or Return belongs to the app's key commands.
    func handle(_ key: UIKey) -> Bool {
        guard key.modifierFlags.intersection([.command, .control, .alternate, .shift]).isEmpty else { return false }
        switch key.keyCode {
        case .keyboardReturnOrEnter, .keypadEnter: return onReturn?() == true
        case .keyboardUpArrow: return onArrowKey?(.up) == true
        case .keyboardDownArrow: return onArrowKey?(.down) == true
        case .keyboardLeftArrow: return onArrowKey?(.left) == true
        case .keyboardRightArrow: return onArrowKey?(.right) == true
        default: return false
        }
    }
}
#endif
