import AppKit

/// Manages syntax highlighting for the code editor.
/// Currently a placeholder — tree-sitter integration (Neon/SwiftTreeSitter SPM packages)
/// will be added in a follow-up step. The editor works with plain monospace text until then.
@MainActor
final class SyntaxHighlightManager {
    private weak var textView: NSTextView?
    private var language: EditorLanguage = .plaintext

    init(textView: NSTextView) {
        self.textView = textView
    }

    func setLanguage(_ language: EditorLanguage) {
        self.language = language
        // TODO: Initialize tree-sitter parser for language
        // TODO: Set up Neon TextViewHighlighter
    }

    func invalidate() {
        // TODO: Re-highlight full document
    }

    func applyHighlighting() {
        // TODO: Use Neon to apply incremental highlighting
        // For now, just ensure the base text color is set
        guard let textView, let textStorage = textView.textStorage else { return }
        let fullRange = NSRange(location: 0, length: textStorage.length)
        textStorage.addAttribute(.foregroundColor, value: NSColor.labelColor, range: fullRange)
        textStorage.addAttribute(
            .font,
            value: NSFont.monospacedSystemFont(ofSize: 13, weight: .regular),
            range: fullRange
        )
    }
}
