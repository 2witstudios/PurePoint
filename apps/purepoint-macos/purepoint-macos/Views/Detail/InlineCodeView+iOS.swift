#if os(iOS)
import SwiftUI
import UIKit

/// iOS twin of the Mac inline code body, with the same inputs and measuring.
struct InlineCodeView: UIViewRepresentable {
    let hunks: [Hunk]
    let language: EditorLanguage

    func makeUIView(context: Context) -> InlineCodeNSView {
        let view = InlineCodeNSView()
        view.update(hunks: hunks, language: language)
        return view
    }

    func updateUIView(_ view: InlineCodeNSView, context: Context) {
        view.update(hunks: hunks, language: language)
    }

    func sizeThatFits(_ proposal: ProposedViewSize, uiView: InlineCodeNSView, context: Context) -> CGSize? {
        guard let width = proposal.width, width > 0 else { return nil }
        return CGSize(width: width, height: uiView.height(for: width))
    }
}

/// Placeholder twin of the Mac inline code view. The Core Text code rendering with selection
/// replaces it (IPAD-6.7).
final class InlineCodeNSView: UIView {
    private var hunks: [Hunk] = []
    private var language: EditorLanguage = .plaintext
    private let placeholderHeight: CGFloat = 44

    init() {
        super.init(frame: .zero)
        clipsToBounds = true
        NotYetOnIPad.addLabel("InlineCodeView", to: self)
    }

    required init?(coder: NSCoder) { fatalError("init(coder:) not supported") }

    func update(hunks: [Hunk], language: EditorLanguage) {
        self.hunks = hunks
        self.language = language
    }

    func height(for width: CGFloat) -> CGFloat {
        placeholderHeight
    }
}
#endif
