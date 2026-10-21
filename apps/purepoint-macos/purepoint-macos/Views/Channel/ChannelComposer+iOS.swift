#if os(iOS)
import SwiftUI
import UIKit

/// Placeholder twin of the Mac channel composer, with the same inputs. The multi-line
/// composer with the Mac's send and newline keys replaces it (IPAD-6.10).
struct ChannelComposer: UIViewRepresentable {
    @Binding var text: String
    var placeholder: String
    var mentions: [String] = []
    var onHeightChanged: (CGFloat) -> Void = { _ in }
    var onSubmit: () -> Void

    func makeUIView(context: Context) -> ComposerTextView {
        let view = ComposerTextView()
        view.submit = onSubmit
        view.mentions = mentions
        view.accessibilityLabel = placeholder
        return view
    }

    func updateUIView(_ view: ComposerTextView, context: Context) {
        view.submit = onSubmit
        view.mentions = mentions
    }
}

/// Placeholder twin of the Mac composer text view, with its submit and mention state.
final class ComposerTextView: UIView {
    var submit: (() -> Void)?
    var mentions: [String] = []
    var completionOpen = false

    init() {
        super.init(frame: .zero)
        NotYetOnIPad.addLabel("ChannelComposer", to: self)
    }

    required init?(coder: NSCoder) { fatalError() }
}
#endif
