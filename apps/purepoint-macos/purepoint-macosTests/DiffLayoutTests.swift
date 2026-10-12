import AppKit
#if !DIFF_LAYOUT_HARNESS
import Testing
@testable import PurePoint
#endif

@MainActor private enum DiffLayoutChecks {
    struct Failure: Error { let message: String }

    static func boundedPageResizes() throws {
        let lines = (1...200).map {
            DiffLine(type: .addition, content: String(repeating: "source text ", count: 8), oldLineNo: nil, newLineNo: $0)
        }
        let view = DiffContentNSView(hunks: [Hunk(header: "@@ -0,0 +1,200 @@", lines: lines)])
        let started = Date()
        let wide = view.height(for: 1000)
        let narrow = view.height(for: 320)
        guard wide > 200, narrow > wide else {
            throw Failure(message: "Diff height must reflect wrapped lines at the actual available width")
        }
        view.update(hunks: [Hunk(header: "@@ -0,0 +1 @@", lines: [lines[0]])])
        let updated = view.height(for: 320)
        guard updated < narrow else { throw Failure(message: "Changed diff content must replace the previous measured height") }
        print("Bounded diff layout/resize checks passed: \(Int(Date().timeIntervalSince(started) * 1000)) ms")
    }
}

#if DIFF_LAYOUT_HARNESS
@main struct DiffLayoutHarness {
    @MainActor static func main() throws { try DiffLayoutChecks.boundedPageResizes() }
}
#else
struct DiffLayoutTests {
    @Test @MainActor func pagesResizeAndReplaceContent() throws { try DiffLayoutChecks.boundedPageResizes() }
}
#endif
