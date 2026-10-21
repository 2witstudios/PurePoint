#if os(iOS)
import UIKit

/// Placeholder twin of the Mac line-number gutter, with the same initializer and rebuild call.
/// The gutter aligned with the iOS editor replaces it (IPAD-6.6).
class LineNumberRulerView: UIView {
    private let gutterWidth: CGFloat = 40
    private weak var textView: UITextView?

    init(textView: UITextView) {
        self.textView = textView
        super.init(frame: CGRect(x: 0, y: 0, width: gutterWidth, height: 0))
        NotYetOnIPad.addLabel("LineNumberRulerView", to: self)
    }

    required init?(coder: NSCoder) { fatalError() }

    func rebuildLineStarts() {}
}
#endif
