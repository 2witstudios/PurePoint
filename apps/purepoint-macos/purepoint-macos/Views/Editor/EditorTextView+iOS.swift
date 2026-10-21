#if os(iOS)
import UIKit

/// iOS twin of the Mac editor text view: a plain-text, TextKit 2 `UITextView` configured as a
/// code editor. ⌘S saves, Tab inserts four spaces, and the system find interaction stands in for
/// the Mac find bar (⌘F, ⌘G, ⇧⌘G).
class EditorTextView: UITextView, UITextViewDelegate {
    var onSave: (() -> Void)?
    var onContentChanged: ((String) -> Void)?
    /// Scrolling or layout moved the text, so views aligned with it (the gutter) redraw.
    var onVisibleTextChanged: (() -> Void)?

    static let font = UIFont.monospacedSystemFont(ofSize: 13, weight: .regular)
    /// The Mac's `textContainerInset` of 4 × 6 points.
    static let inset = UIEdgeInsets(top: 6, left: 4, bottom: 6, right: 4)
    static let tabReplacement = "    "

    override init(frame: CGRect, textContainer container: NSTextContainer?) {
        super.init(frame: frame, textContainer: container)
        commonInit()
    }

    required init?(coder: NSCoder) {
        super.init(coder: coder)
        commonInit()
    }

    private func commonInit() {
        font = Self.font
        textColor = .labelColor
        typingAttributes = [.font: Self.font, .foregroundColor: NSColor.labelColor]
        isEditable = true
        isSelectable = true
        allowsEditingTextAttributes = false
        dataDetectorTypes = []
        autocorrectionType = .no
        autocapitalizationType = .none
        spellCheckingType = .no
        smartQuotesType = .no
        smartDashesType = .no
        smartInsertDeleteType = .no
        inlinePredictionType = .no
        writingToolsBehavior = .none
        isFindInteractionEnabled = true
        alwaysBounceVertical = true
        showsHorizontalScrollIndicator = false
        keyboardDismissMode = .interactive
        textContainerInset = Self.inset
        backgroundColor = Theme.cardBackground
        delegate = self
    }

    // MARK: - Keyboard

    override var keyCommands: [UIKeyCommand]? {
        let save = UIKeyCommand(
            title: "Save", action: #selector(saveDocument(_:)), input: "s", modifierFlags: .command)
        save.wantsPriorityOverSystemBehavior = true
        return (super.keyCommands ?? []) + [save]
    }

    @objc func saveDocument(_ sender: Any?) {
        onSave?()
    }

    override func canPerformAction(_ action: Selector, withSender sender: Any?) -> Bool {
        if action == #selector(saveDocument(_:)) { return true }
        return super.canPerformAction(action, withSender: sender)
    }

    /// Tab inserts four spaces, as on the Mac.
    override func insertText(_ text: String) {
        super.insertText(text == "\t" ? Self.tabReplacement : text)
    }

    // MARK: - Content

    /// Replace the whole text without reporting an edit. The undo stack belonged to the old
    /// text, so it is cleared rather than left pointing at ranges that no longer exist.
    func replaceAllText(_ string: String) {
        text = string
        undoManager?.removeAllActions()
    }

    func textViewDidChange(_ textView: UITextView) {
        onContentChanged?(text)
    }

    func scrollViewDidScroll(_ scrollView: UIScrollView) {
        onVisibleTextChanged?()
    }

    override func layoutSubviews() {
        super.layoutSubviews()
        onVisibleTextChanged?()
    }
}
#endif
