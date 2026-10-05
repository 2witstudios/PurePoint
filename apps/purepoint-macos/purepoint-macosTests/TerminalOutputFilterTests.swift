import Foundation
import Testing

@testable import PurePoint

struct TerminalOutputFilterTests {

    private func filter(_ s: String, rows: Int = 24) -> String {
        let out = DaemonAttachSession.filterTerminalOutput(Array(s.utf8), maxRows: rows)
        return String(decoding: out, as: UTF8.self)
    }

    @Test func plainTextPassesThroughUnchanged() {
        #expect(filter("hello world\r\n") == "hello world\r\n")
    }

    @Test func stripsSynchronizedOutputBeginAndEnd() {
        #expect(filter("a\u{1b}[?2026hb\u{1b}[?2026lc") == "abc")
    }

    @Test func keepsOtherPrivateModes() {
        #expect(filter("\u{1b}[?2025h\u{1b}[?1049h") == "\u{1b}[?2025h\u{1b}[?1049h")
    }

    @Test func clampsCursorUpToViewport() {
        #expect(filter("\u{1b}[100A", rows: 10) == "\u{1b}[9A")
    }

    @Test func leavesSmallCursorUpAlone() {
        #expect(filter("\u{1b}[3A", rows: 10) == "\u{1b}[3A")
    }

    @Test func leavesOtherCSISequencesAlone() {
        #expect(filter("\u{1b}[31mred\u{1b}[0m\u{1b}[2B") == "\u{1b}[31mred\u{1b}[0m\u{1b}[2B")
    }

    @Test func handlesTruncatedSequenceAtChunkEnd() {
        #expect(filter("x\u{1b}[?202") == "x\u{1b}[?202")
        #expect(filter("\u{1b}") == "\u{1b}")
    }
}
