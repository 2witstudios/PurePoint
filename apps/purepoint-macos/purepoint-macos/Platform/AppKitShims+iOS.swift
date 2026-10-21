#if os(iOS)
import SwiftUI
import UIKit

// AppKit value types under their Mac names, so shared code that only needs colors, fonts and
// images compiles on iOS unchanged. AppKit *view* classes are not shimmed; they get real UIKit
// twins (`<Name>+iOS.swift`).

typealias NSColor = UIColor
typealias NSFont = UIFont
typealias NSImage = UIImage

// MARK: - NSColor

nonisolated extension UIColor {
    /// AppKit's sRGB initializer. `UIColor(red:green:blue:alpha:)` is extended sRGB, which is
    /// the same color for in-gamut components.
    convenience init(srgbRed red: CGFloat, green: CGFloat, blue: CGFloat, alpha: CGFloat) {
        self.init(red: red, green: green, blue: blue, alpha: alpha)
    }

    /// AppKit converts a color into a component-based color space before `getRed`. UIKit's
    /// `getRed` already converts any RGB-compatible color, so the color is returned as is.
    func usingColorSpace(_ space: NSColorSpace) -> UIColor? { self }

    // System colors under their AppKit names, mapped to the closest UIKit semantic color.
    static var labelColor: UIColor { .label }
    static var secondaryLabelColor: UIColor { .secondaryLabel }
    static var tertiaryLabelColor: UIColor { .tertiaryLabel }
    static var quaternaryLabelColor: UIColor { .quaternaryLabel }
    static var placeholderTextColor: UIColor { .placeholderText }
    static var textColor: UIColor { .label }
    static var textBackgroundColor: UIColor { .systemBackground }
    static var windowBackgroundColor: UIColor { .systemBackground }
    static var controlBackgroundColor: UIColor { .secondarySystemBackground }
    static var underPageBackgroundColor: UIColor { .systemGroupedBackground }
    static var separatorColor: UIColor { .separator }
    static var gridColor: UIColor { .separator }
    static var controlAccentColor: UIColor { .tintColor }
    static var selectedContentBackgroundColor: UIColor { .tintColor }
    static var unemphasizedSelectedContentBackgroundColor: UIColor { .systemFill }
    static var linkColor: UIColor { .link }
}

/// Stand-in for AppKit's color spaces, accepted by `usingColorSpace(_:)`.
nonisolated struct NSColorSpace: Sendable {
    static let deviceRGB = NSColorSpace()
    static let sRGB = NSColorSpace()
    static let genericRGB = NSColorSpace()
}

// MARK: - NSImage

nonisolated extension UIImage {
    /// AppKit's SF Symbol initializer. UIKit images carry no accessibility description; views
    /// set their own accessibility label.
    convenience init?(systemSymbolName name: String, accessibilityDescription: String?) {
        self.init(systemName: name)
    }

    /// AppKit's bitmap initializer. UIKit sizes the image from its pixels at scale 1, which is
    /// what the callers pass as `size`.
    convenience init(cgImage: CGImage, size: CGSize) {
        self.init(cgImage: cgImage, scale: 1, orientation: .up)
    }
}

// MARK: - SwiftUI bridges

extension Color {
    init(nsColor: NSColor) {
        self.init(uiColor: nsColor)
    }
}

extension Image {
    init(nsImage: NSImage) {
        self.init(uiImage: nsImage)
    }
}
#endif
