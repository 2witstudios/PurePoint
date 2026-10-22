#if os(iOS)
import SwiftUI

// The Mac's text-style metrics under SwiftUI's names, so shared views that write
// `.font(.headline)` or `.font(.caption)` draw at the sizes they have on the Mac. iOS text
// styles run 2–5pt larger (body is 17pt there, 13pt on the Mac), which wraps and clips layouts
// sized for the Mac. Declarations in this module shadow SwiftUI's, so call sites don't change.
// The app root also sets `.body` as the default font, the Mac's default for unstyled text.
extension Font {
    static let largeTitle = Font.system(size: 26)
    static let title = Font.system(size: 22)
    static let title2 = Font.system(size: 17)
    static let title3 = Font.system(size: 15)
    static let headline = Font.system(size: 13, weight: .bold)
    static let subheadline = Font.system(size: 11)
    static let body = Font.system(size: 13)
    static let callout = Font.system(size: 12)
    static let footnote = Font.system(size: 10)
    static let caption = Font.system(size: 10)
    static let caption2 = Font.system(size: 10, weight: .medium)
}
#endif
