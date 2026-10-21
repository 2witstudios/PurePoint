#if os(iOS)
import UIKit

/// Placeholder twin of the Mac editor text view, with the same callbacks and initializer. The
/// TextKit 2 `UITextView` editor replaces it (IPAD-6.5).
class EditorTextView: UIView {
    var onSave: (() -> Void)?
    var onContentChanged: ((String) -> Void)?

    init(frame: CGRect, textContainer container: NSTextContainer?) {
        super.init(frame: frame)
        NotYetOnIPad.addLabel("EditorTextView", to: self)
    }

    required init?(coder: NSCoder) { fatalError() }
}
#endif
