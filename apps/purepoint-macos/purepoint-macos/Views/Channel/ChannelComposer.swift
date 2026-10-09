import AppKit
import SwiftUI

/// NSTextView preserves IME composition and native editing/selection shortcuts.
struct ChannelComposer: NSViewRepresentable {
    @Binding var text: String
    var placeholder: String
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
    }
    func makeCoordinator() -> Coordinator { Coordinator(self) }
    final class Coordinator: NSObject, NSTextViewDelegate {
        var parent: ChannelComposer
        init(_ parent: ChannelComposer) { self.parent = parent }
        func textDidChange(_ notification: Notification) {
            guard let view = notification.object as? NSTextView else { return }
            parent.text = view.string
        }
    }
}
final class ComposerTextView: NSTextView {
    var submit: (() -> Void)?
    override func keyDown(with event: NSEvent) {
        if event.keyCode == 36, !event.modifierFlags.contains(.shift), !hasMarkedText() {
            submit?()
        } else { super.keyDown(with: event) }
    }
}
