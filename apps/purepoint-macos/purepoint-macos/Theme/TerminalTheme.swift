import AppKit
import SwiftTerm

enum TerminalTheme {
    static let background = NSColor(red: 0.11, green: 0.11, blue: 0.12, alpha: 1)
    static let foreground = NSColor(red: 0.85, green: 0.85, blue: 0.87, alpha: 1)

    static func ansiPalette(for appearance: NSAppearance) -> [SwiftTerm.Color] {
        let isDark = appearance.bestMatch(from: [.darkAqua, .aqua]) == .darkAqua
        let palette: [(CGFloat, CGFloat, CGFloat)] = isDark ? [
            (0.11, 0.11, 0.12), // 0 black
            (0.78, 0.35, 0.35), // 1 red
            (0.43, 0.68, 0.47), // 2 green
            (0.79, 0.67, 0.40), // 3 yellow
            (0.43, 0.61, 0.90), // 4 blue
            (0.72, 0.53, 0.86), // 5 magenta
            (0.39, 0.72, 0.78), // 6 cyan
            (0.68, 0.68, 0.70), // 7 white
            (0.34, 0.35, 0.37), // 8 bright black
            (0.89, 0.48, 0.48), // 9 bright red
            (0.55, 0.84, 0.60), // 10 bright green
            (0.90, 0.78, 0.50), // 11 bright yellow
            (0.54, 0.70, 0.94), // 12 bright blue
            (0.80, 0.63, 0.91), // 13 bright magenta
            (0.49, 0.79, 0.84), // 14 bright cyan
            (0.94, 0.94, 0.95), // 15 bright white
        ] : [
            (0.17, 0.17, 0.18), // 0 black
            (0.71, 0.23, 0.23), // 1 red
            (0.18, 0.49, 0.20), // 2 green
            (0.55, 0.43, 0.12), // 3 yellow
            (0.18, 0.37, 0.69), // 4 blue
            (0.48, 0.25, 0.64), // 5 magenta
            (0.12, 0.44, 0.52), // 6 cyan
            (0.87, 0.86, 0.82), // 7 white
            (0.43, 0.43, 0.45), // 8 bright black
            (0.82, 0.31, 0.31), // 9 bright red
            (0.25, 0.58, 0.30), // 10 bright green
            (0.64, 0.52, 0.18), // 11 bright yellow
            (0.25, 0.47, 0.78), // 12 bright blue
            (0.58, 0.35, 0.75), // 13 bright magenta
            (0.20, 0.54, 0.63), // 14 bright cyan
            (0.99, 0.99, 0.99), // 15 bright white
        ]

        return palette.map { makeColor(red: $0.0, green: $0.1, blue: $0.2) }
    }

    static func makeColor(red: CGFloat, green: CGFloat, blue: CGFloat) -> SwiftTerm.Color {
        let r = UInt16((max(0, min(1, red)) * 65535).rounded())
        let g = UInt16((max(0, min(1, green)) * 65535).rounded())
        let b = UInt16((max(0, min(1, blue)) * 65535).rounded())
        return SwiftTerm.Color(red: r, green: g, blue: b)
    }

    static func makeColor(from nsColor: NSColor) -> SwiftTerm.Color {
        let rgb = nsColor.usingColorSpace(.deviceRGB) ?? nsColor
        var r: CGFloat = 0, g: CGFloat = 0, b: CGFloat = 0, a: CGFloat = 0
        rgb.getRed(&r, green: &g, blue: &b, alpha: &a)
        return makeColor(red: r, green: g, blue: b)
    }

    static func apply(to terminalView: LocalProcessTerminalView, appearance: NSAppearance) {
        let bg = background
        let fg = foreground
        let term = terminalView.getTerminal()

        term.backgroundColor = makeColor(from: bg)
        term.foregroundColor = makeColor(from: fg)

        terminalView.nativeBackgroundColor = bg
        terminalView.nativeForegroundColor = fg
        terminalView.installColors(ansiPalette(for: appearance))
        terminalView.layer?.backgroundColor = bg.cgColor

        terminalView.colorChanged(source: term, idx: nil)
        term.refresh(startRow: 0, endRow: term.rows - 1)
        terminalView.needsDisplay = true
        terminalView.font = terminalView.font
    }
}
