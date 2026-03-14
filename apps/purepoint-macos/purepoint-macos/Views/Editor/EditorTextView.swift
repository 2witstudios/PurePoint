import AppKit

class EditorTextView: NSTextView {
    var onSave: (() -> Void)?
    var onContentChanged: ((String) -> Void)?

    override init(frame frameRect: NSRect, textContainer container: NSTextContainer?) {
        super.init(frame: frameRect, textContainer: container)
        commonInit()
    }

    required init?(coder: NSCoder) {
        super.init(coder: coder)
        commonInit()
    }

    private func commonInit() {
        font = NSFont.monospacedSystemFont(ofSize: 13, weight: .regular)
        isEditable = true
        isSelectable = true
        isRichText = false
        allowsUndo = true
        isAutomaticQuoteSubstitutionEnabled = false
        isAutomaticDashSubstitutionEnabled = false
        isAutomaticTextReplacementEnabled = false
        isAutomaticSpellingCorrectionEnabled = false
        isAutomaticTextCompletionEnabled = false
        usesFindBar = true
        isIncrementalSearchingEnabled = true
        drawsBackground = true
        textContainerInset = NSSize(width: 4, height: 6)
    }

    override func keyDown(with event: NSEvent) {
        // Cmd+S → save
        if event.modifierFlags.contains(.command) && event.charactersIgnoringModifiers == "s" {
            onSave?()
            return
        }

        super.keyDown(with: event)
    }

    override func insertTab(_ sender: Any?) {
        insertText("    ", replacementRange: selectedRange())
    }

    override func didChangeText() {
        super.didChangeText()
        onContentChanged?(string)
    }
}
