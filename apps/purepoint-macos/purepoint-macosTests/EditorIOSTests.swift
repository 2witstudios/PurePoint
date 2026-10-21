#if os(iOS)
import Foundation
import Testing
import UIKit

@testable import PurePoint

/// The iPad editor twin: a TextKit 2 code editor with the Mac's behaviour, its line-number
/// gutter, Neon highlighting, and the Markdown preview's link handling.
@MainActor
@Suite struct EditorIOSTests {
    // MARK: - Gutter

    @Test func lineStartsSkipTheTrailingNewline() {
        #expect(LineNumberRulerView.lineStarts(in: "") == [0])
        #expect(LineNumberRulerView.lineStarts(in: "a") == [0])
        #expect(LineNumberRulerView.lineStarts(in: "a\n") == [0])
        #expect(LineNumberRulerView.lineStarts(in: "a\nbc\n\nd") == [0, 2, 5, 6])
        #expect(LineNumberRulerView.lineStarts(in: "a\r\nb") == [0, 3])
    }

    @Test func lineIndexFindsTheLineHoldingACharacter() {
        let starts = [0, 2, 5, 6]
        #expect(LineNumberRulerView.lineIndex(forCharacter: 0, in: starts) == 0)
        #expect(LineNumberRulerView.lineIndex(forCharacter: 1, in: starts) == 0)
        #expect(LineNumberRulerView.lineIndex(forCharacter: 2, in: starts) == 1)
        #expect(LineNumberRulerView.lineIndex(forCharacter: 4, in: starts) == 1)
        #expect(LineNumberRulerView.lineIndex(forCharacter: 5, in: starts) == 2)
        #expect(LineNumberRulerView.lineIndex(forCharacter: 99, in: starts) == 3)
    }

}
#endif
