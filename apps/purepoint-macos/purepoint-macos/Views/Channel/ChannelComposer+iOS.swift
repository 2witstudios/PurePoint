#if os(iOS)
import SwiftUI
import UIKit

/// iOS twin of the Mac channel composer: a multi-line `UITextView` that keeps IME composition
/// and native editing, sends on Return and inserts a newline on Shift-Return, and completes
/// `@` mentions from a list under the caret.
struct ChannelComposer: UIViewRepresentable {
    @Binding var text: String
    var placeholder: String
    var mentions: [String] = []
    var onHeightChanged: (CGFloat) -> Void = { _ in }
    var onSubmit: () -> Void

    func makeUIView(context: Context) -> ComposerTextView {
        let view = ComposerTextView(usingTextLayoutManager: true)
        view.font = .systemFont(ofSize: 13)
        view.textColor = .labelColor
        view.backgroundColor = .clear
        // NSTextView's 10 × 8 inset; both keep the default 5-point line fragment padding.
        view.textContainerInset = UIEdgeInsets(top: 8, left: 10, bottom: 8, right: 10)
        view.isScrollEnabled = true
        view.showsVerticalScrollIndicator = true
        view.returnKeyType = .send
        view.delegate = context.coordinator
        view.submit = onSubmit
        view.mentions = mentions
        view.accessibilityLabel = placeholder
        return view
    }

    func updateUIView(_ view: ComposerTextView, context: Context) {
        context.coordinator.parent = self
        if view.text != text, view.markedTextRange == nil { view.text = text }
        view.submit = onSubmit
        view.mentions = mentions
        context.coordinator.updateHeight(view)
    }

    func makeCoordinator() -> Coordinator { Coordinator(self) }

    final class Coordinator: NSObject, UITextViewDelegate {
        var parent: ChannelComposer
        init(_ parent: ChannelComposer) { self.parent = parent }

        func textViewDidChange(_ textView: UITextView) {
            parent.text = textView.text
            updateHeight(textView)
            if let editor = textView as? ComposerTextView, editor.markedTextRange == nil,
                editor.rangeForUserCompletion.location != NSNotFound
            {
                editor.complete()
            } else {
                (textView as? ComposerTextView)?.closeCompletion()
            }
        }

        func textViewDidChangeSelection(_ textView: UITextView) {
            guard let editor = textView as? ComposerTextView, editor.completionOpen,
                editor.rangeForUserCompletion.location == NSNotFound
            else { return }
            editor.closeCompletion()
        }

        func textViewDidEndEditing(_ textView: UITextView) {
            (textView as? ComposerTextView)?.closeCompletion()
        }

        /// The software keyboard's Return (and any hardware Return the view did not take)
        /// arrives as a "\n" insertion: it sends, as Return does on the Mac.
        func textView(
            _ textView: UITextView, shouldChangeTextIn range: NSRange, replacementText text: String
        ) -> Bool {
            guard text == "\n", let editor = textView as? ComposerTextView, !editor.insertingNewline else {
                return true
            }
            return !editor.handleReturn(shift: false)
        }

        func updateHeight(_ view: UITextView) {
            let width = view.bounds.width > 0 ? view.bounds.width : 320
            let fitted = view.sizeThatFits(CGSize(width: width, height: .greatestFiniteMagnitude)).height
            let used = fitted - view.textContainerInset.top - view.textContainerInset.bottom
            let height = min(180, max(60, used + 18))
            DispatchQueue.main.async { [parent] in parent.onHeightChanged(height) }
        }
    }
}

/// iOS twin of the Mac composer text view: the same submit and mention state, Return and
/// Shift-Return keys, and `@` completion range.
final class ComposerTextView: UITextView {
    var submit: (() -> Void)?
    var mentions: [String] = []
    var completionOpen: Bool { completionList != nil }
    /// Set while the view inserts its own newline, so the delegate lets it through.
    private(set) var insertingNewline = false
    private var completionList: MentionCompletionList?
    private var consumedKeys: Set<UIKeyboardHIDUsage> = []

    /// The `@token` ending at the caret, or `NSNotFound` when the caret is not in one.
    var rangeForUserCompletion: NSRange {
        let cursor = selectedRange.location
        let string = (text ?? "") as NSString
        guard cursor != NSNotFound, cursor <= string.length else { return NSRange(location: NSNotFound, length: 0) }
        let prefix = string.substring(to: cursor) as NSString
        let at = prefix.range(of: "@", options: .backwards)
        guard at.location != NSNotFound else { return NSRange(location: NSNotFound, length: 0) }
        let token = prefix.substring(from: at.location)
        guard !token.contains(where: { $0.isWhitespace }) else { return NSRange(location: NSNotFound, length: 0) }
        return NSRange(location: at.location, length: cursor - at.location)
    }

    /// The mentions completing `prefix` (which starts with "@"), case-insensitively.
    func completions(forPartialWord prefix: String) -> [String] {
        guard prefix.hasPrefix("@") else { return [] }
        return mentions.map { "@" + $0 }.filter { $0.lowercased().hasPrefix(prefix.lowercased()) }
    }

    /// Open, refresh or close the completion list for the token at the caret.
    func complete() {
        let range = rangeForUserCompletion
        guard range.location != NSNotFound else { return closeCompletion() }
        let matches = completions(forPartialWord: ((text ?? "") as NSString).substring(with: range))
        guard !matches.isEmpty, let window else { return closeCompletion() }
        let list = completionList ?? MentionCompletionList { [weak self] word in
            self?.insertCompletion(word, forPartialWordRange: self?.rangeForUserCompletion, isFinal: true)
        }
        list.words = matches
        if list.superview == nil { window.addSubview(list) }
        completionList = list
        positionCompletionList(list, in: window)
    }

    func closeCompletion() {
        completionList?.removeFromSuperview()
        completionList = nil
    }

    /// Replace the partial `@token` with the chosen mention.
    func insertCompletion(_ word: String, forPartialWordRange range: NSRange?, isFinal: Bool) {
        guard let range, range.location != NSNotFound,
            let start = position(from: beginningOfDocument, offset: range.location),
            let end = position(from: start, offset: range.length),
            let textRange = textRange(from: start, to: end)
        else { return closeCompletion() }
        replace(textRange, withText: word)
        if isFinal { closeCompletion() }
    }

    /// Return sends unless Shift is held, IME text is being composed, or the completion list is
    /// open (where Return picks the highlighted mention). Returns whether Return was consumed.
    @discardableResult
    func handleReturn(shift: Bool) -> Bool {
        if markedTextRange != nil { return false }
        if let completionList {
            insertCompletion(completionList.selectedWord, forPartialWordRange: rangeForUserCompletion, isFinal: true)
            return true
        }
        if shift {
            insertNewline()
        } else {
            submit?()
        }
        return true
    }

    private func insertNewline() {
        insertingNewline = true
        insertText("\n")
        insertingNewline = false
    }

    override func pressesBegan(_ presses: Set<UIPress>, with event: UIPressesEvent?) {
        guard let key = presses.first?.key, markedTextRange == nil else {
            return super.pressesBegan(presses, with: event)
        }
        if handleKey(key) {
            consumedKeys.insert(key.keyCode)
            return
        }
        super.pressesBegan(presses, with: event)
    }

    override func pressesEnded(_ presses: Set<UIPress>, with event: UIPressesEvent?) {
        guard let code = presses.first?.key?.keyCode, consumedKeys.remove(code) != nil else {
            return super.pressesEnded(presses, with: event)
        }
    }

    override func pressesCancelled(_ presses: Set<UIPress>, with event: UIPressesEvent?) {
        guard let code = presses.first?.key?.keyCode, consumedKeys.remove(code) != nil else {
            return super.pressesCancelled(presses, with: event)
        }
    }

    /// The composer's own keys. Returns whether `key` was handled.
    func handleKey(_ key: UIKey) -> Bool {
        switch key.keyCode {
        case .keyboardReturnOrEnter, .keypadEnter:
            return handleReturn(shift: key.modifierFlags.contains(.shift))
        case .keyboardUpArrow where completionOpen:
            completionList?.moveSelection(by: -1)
            return true
        case .keyboardDownArrow where completionOpen:
            completionList?.moveSelection(by: 1)
            return true
        case .keyboardTab where completionOpen:
            return handleReturn(shift: false)
        case .keyboardEscape:
            // Escape closes an open list; otherwise it asks for completions, as on the Mac.
            if completionOpen { closeCompletion() } else { complete() }
            return true
        default:
            return false
        }
    }

    override func layoutSubviews() {
        super.layoutSubviews()
        if let completionList, let window { positionCompletionList(completionList, in: window) }
    }

    override func removeFromSuperview() {
        closeCompletion()
        super.removeFromSuperview()
    }

    /// Under the caret, or above it when the window has no room below (the channel composer
    /// sits at the bottom of the window).
    private func positionCompletionList(_ list: MentionCompletionList, in window: UIWindow) {
        let caret = selectedTextRange.map { caretRect(for: $0.end) } ?? .zero
        let anchor = convert(caret, to: window)
        let size = list.fittingSize
        var origin = CGPoint(x: anchor.minX, y: anchor.maxY + 2)
        if origin.y + size.height > window.bounds.maxY - 8 { origin.y = anchor.minY - size.height - 2 }
        origin.x = min(origin.x, window.bounds.maxX - size.width - 8)
        list.frame = CGRect(origin: origin, size: size)
    }
}

/// The `@` mention list: a small floating menu of matches with a highlighted row, driven by the
/// arrow keys from the composer and by taps or clicks.
final class MentionCompletionList: UIView {
    var words: [String] = [] {
        didSet {
            if selectedIndex >= words.count { selectedIndex = 0 }
            rebuild()
        }
    }
    private(set) var selectedIndex = 0
    private let onPick: (String) -> Void
    private let stack = UIStackView()
    private static let rowHeight: CGFloat = 22
    private static let maxRows = 8

    var selectedWord: String { words.indices.contains(selectedIndex) ? words[selectedIndex] : "" }

    var fittingSize: CGSize {
        let font = UIFont.systemFont(ofSize: 13)
        let widest = words.map { ($0 as NSString).size(withAttributes: [.font: font]).width }.max() ?? 0
        return CGSize(
            width: min(320, max(140, ceil(widest) + 24)),
            height: CGFloat(min(words.count, Self.maxRows)) * Self.rowHeight + 8)
    }

    init(onPick: @escaping (String) -> Void) {
        self.onPick = onPick
        super.init(frame: .zero)
        backgroundColor = .secondarySystemBackground
        layer.cornerRadius = 6
        layer.borderWidth = 1
        layer.borderColor = UIColor.separator.cgColor
        layer.shadowColor = UIColor.black.cgColor
        layer.shadowOpacity = 0.2
        layer.shadowRadius = 8
        layer.shadowOffset = CGSize(width: 0, height: 3)
        stack.axis = .vertical
        stack.translatesAutoresizingMaskIntoConstraints = false
        addSubview(stack)
        NSLayoutConstraint.activate([
            stack.topAnchor.constraint(equalTo: topAnchor, constant: 4),
            stack.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 4),
            stack.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -4),
        ])
    }

    required init?(coder: NSCoder) { fatalError("init(coder:) not supported") }

    func moveSelection(by offset: Int) {
        guard !words.isEmpty else { return }
        selectedIndex = (selectedIndex + offset + words.count) % words.count
        rebuild()
    }

    private func rebuild() {
        stack.arrangedSubviews.forEach { $0.removeFromSuperview() }
        let visible = visibleRange
        for index in visible {
            var config = UIButton.Configuration.plain()
            config.title = words[index]
            config.contentInsets = NSDirectionalEdgeInsets(top: 0, leading: 8, bottom: 0, trailing: 8)
            config.titleTextAttributesTransformer = UIConfigurationTextAttributesTransformer { attributes in
                var attributes = attributes
                attributes.font = .systemFont(ofSize: 13)
                return attributes
            }
            let selected = index == selectedIndex
            config.baseForegroundColor = selected ? .white : .label
            config.background.backgroundColor = selected ? .tintColor : .clear
            config.background.cornerRadius = 4
            let button = UIButton(
                configuration: config,
                primaryAction: UIAction { [weak self] _ in
                    guard let self else { return }
                    self.onPick(self.words[index])
                })
            button.contentHorizontalAlignment = .leading
            button.heightAnchor.constraint(equalToConstant: Self.rowHeight).isActive = true
            stack.addArrangedSubview(button)
        }
    }

    /// The window of at most `maxRows` rows that keeps the selection visible.
    private var visibleRange: Range<Int> {
        let start = min(max(0, selectedIndex - Self.maxRows + 1), max(0, words.count - Self.maxRows))
        return start..<min(words.count, start + Self.maxRows)
    }
}
#endif
