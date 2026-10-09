import AppKit
import SwiftUI

/// NSTextView preserves IME composition and native editing/selection shortcuts.
struct ChannelComposer: NSViewRepresentable {
    @Binding var text: String
    var placeholder: String
    var mentions: [String] = []
    var onHeightChanged: (CGFloat) -> Void = { _ in }
    var onSubmit: () -> Void
    func makeNSView(context: Context) -> NSScrollView {
        let scroll = NSScrollView()
        let view = ComposerTextView()
        view.isRichText = false
        view.font = .systemFont(ofSize: 13)
        view.textColor = .labelColor
        view.backgroundColor = .clear
        view.drawsBackground = false
        view.textContainerInset = NSSize(width: 10, height: 8)
        view.isVerticallyResizable = true
        view.autoresizingMask = [.width]
        view.textContainer?.widthTracksTextView = true
        view.delegate = context.coordinator
        view.submit = onSubmit
        view.mentions = mentions
        view.setAccessibilityLabel(placeholder)
        scroll.documentView = view
        scroll.hasVerticalScroller = true
        scroll.drawsBackground = false
        return scroll
    }
    func updateNSView(_ view: NSScrollView, context: Context) {
        context.coordinator.parent = self
        guard let editor = view.documentView as? ComposerTextView else { return }
        if editor.string != text, !editor.hasMarkedText() { editor.string = text }
        editor.submit = onSubmit
        editor.mentions = mentions
        context.coordinator.updateHeight(editor)
    }
    func makeCoordinator() -> Coordinator { Coordinator(self) }
    final class Coordinator: NSObject, NSTextViewDelegate {
        var parent: ChannelComposer
        init(_ parent: ChannelComposer) { self.parent = parent }
        func textDidChange(_ notification: Notification) {
            guard let view = notification.object as? NSTextView else { return }
            parent.text = view.string
            updateHeight(view)
            if let editor = view as? ComposerTextView, !editor.hasMarkedText(), editor.rangeForUserCompletion.location != NSNotFound {
                editor.complete(nil)
            }
        }
        func updateHeight(_ view: NSTextView) {
            guard let container = view.textContainer, let layout = view.layoutManager else { return }
            layout.ensureLayout(for: container)
            let height = min(180, max(60, layout.usedRect(for: container).height + 18))
            DispatchQueue.main.async { [parent] in parent.onHeightChanged(height) }
        }
        func textView(_ textView: NSTextView, completions words: [String], forPartialWordRange range: NSRange, indexOfSelectedItem index: UnsafeMutablePointer<Int>?) -> [String] {
            let prefix = (textView.string as NSString).substring(with: range)
            guard prefix.hasPrefix("@") else { return [] }
            let matches = parent.mentions.map { "@" + $0 }.filter { $0.lowercased().hasPrefix(prefix.lowercased()) }
            (textView as? ComposerTextView)?.completionOpen = !matches.isEmpty
            index?.pointee = 0
            return matches
        }
    }
}
final class ComposerTextView: NSTextView {
    var submit: (() -> Void)?
    var mentions: [String] = []
    var completionOpen = false
    override var rangeForUserCompletion: NSRange {
        let cursor = selectedRange().location
        guard cursor != NSNotFound, cursor <= (string as NSString).length else { return NSRange(location: NSNotFound, length: 0) }
        let prefix = (string as NSString).substring(to: cursor) as NSString
        let at = prefix.range(of: "@", options: .backwards)
        guard at.location != NSNotFound else { return NSRange(location: NSNotFound, length: 0) }
        let token = prefix.substring(from: at.location)
        guard !token.contains(where: { $0.isWhitespace }) else { return NSRange(location: NSNotFound, length: 0) }
        return NSRange(location: at.location, length: cursor - at.location)
    }
    override func insertCompletion(_ word: String, forPartialWordRange charRange: NSRange, movement: Int, isFinal flag: Bool) {
        super.insertCompletion(word, forPartialWordRange: charRange, movement: movement, isFinal: flag)
        if flag { completionOpen = false }
    }
    override func keyDown(with event: NSEvent) {
        if (event.keyCode == 36 || event.keyCode == 76), !event.modifierFlags.contains(.shift), !hasMarkedText(), !completionOpen {
            submit?()
        } else { super.keyDown(with: event); if event.keyCode == 53 { completionOpen = false } }
    }
}
