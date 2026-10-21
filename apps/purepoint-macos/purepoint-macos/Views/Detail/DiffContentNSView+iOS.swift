#if os(iOS)
import SwiftUI
import UIKit

/// Placeholder twin of the Mac diff body view, with the same initializer, update and measuring
/// API. The Core Text diff rendering replaces it (IPAD-6.7).
class DiffContentNSView: UIView {
    private var hunks: [Hunk] = []
    private let placeholderHeight: CGFloat = 44

    init(hunks: [Hunk]) {
        self.hunks = hunks
        super.init(frame: .zero)
        NotYetOnIPad.addLabel("DiffContentNSView", to: self)
    }

    required init?(coder: NSCoder) {
        fatalError("init(coder:) not supported")
    }

    func update(hunks: [Hunk]) {
        self.hunks = hunks
    }

    func height(for width: CGFloat) -> CGFloat {
        placeholderHeight
    }

    override var intrinsicContentSize: CGSize {
        CGSize(width: UIView.noIntrinsicMetric, height: placeholderHeight)
    }
}

// MARK: - SwiftUI Bridge

struct DiffContentRepresentable: UIViewRepresentable {
    let hunks: [Hunk]

    func makeUIView(context: Context) -> DiffContentNSView {
        DiffContentNSView(hunks: hunks)
    }

    func sizeThatFits(_ proposal: ProposedViewSize, uiView: DiffContentNSView, context: Context) -> CGSize? {
        guard let width = proposal.width, width > 0 else { return nil }
        return CGSize(width: width, height: uiView.height(for: width))
    }

    func updateUIView(_ uiView: DiffContentNSView, context: Context) {
        uiView.update(hunks: hunks)
    }
}
#endif
