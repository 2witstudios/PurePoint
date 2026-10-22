#if os(iOS)
import Foundation
import Testing
import UIKit

@testable import PurePoint

/// The iPad editor twin: a TextKit 2 code editor with the Mac's behaviour, its line-number
/// gutter, Neon highlighting, and the Markdown preview's link handling.
@MainActor
@Suite struct EditorIOSTests {
    private func makeEditor(_ content: String = "", language: EditorLanguage = .plaintext) -> EditorNSView {
        let editor = EditorNSView(frame: CGRect(x: 0, y: 0, width: 600, height: 400))
        editor.setContent(content)
        editor.setLanguage(language)
        editor.layoutIfNeeded()
        return editor
    }

    // MARK: - Text view

    @Test func usesTextKit2WithoutFallingBack() {
        let textView = makeEditor("let a = 1\n").editorTextView
        #expect(textView.textLayoutManager != nil)
    }

    @Test func isConfiguredAsACodeEditor() {
        let textView = makeEditor().editorTextView
        #expect(textView.font == UIFont.monospacedSystemFont(ofSize: 13, weight: .regular))
        #expect(textView.autocorrectionType == .no)
        #expect(textView.autocapitalizationType == .none)
        #expect(textView.spellCheckingType == .no)
        #expect(textView.smartQuotesType == .no)
        #expect(textView.smartDashesType == .no)
        #expect(textView.smartInsertDeleteType == .no)
        #expect(textView.isFindInteractionEnabled)
        #expect(!textView.allowsEditingTextAttributes)
        #expect(textView.textContainerInset == UIEdgeInsets(top: 6, left: 4, bottom: 6, right: 4))
    }

    @Test func setContentDoesNotReportAnEdit() {
        let editor = makeEditor()
        var reported: [String] = []
        editor.onContentChanged = { reported.append($0) }
        editor.setContent("fresh")
        #expect(editor.getContent() == "fresh")
        #expect(reported.isEmpty)
    }

    @Test func typingReportsTheNewContent() {
        let editor = makeEditor("a")
        var reported: [String] = []
        editor.onContentChanged = { reported.append($0) }
        let textView = editor.editorTextView
        textView.selectedRange = NSRange(location: 1, length: 0)
        textView.insertText("b")
        #expect(editor.getContent() == "ab")
        #expect(reported.last == "ab")
    }

    @Test func tabInsertsFourSpaces() {
        let editor = makeEditor("x")
        let textView = editor.editorTextView
        textView.selectedRange = NSRange(location: 0, length: 0)
        textView.insertText("\t")
        #expect(editor.getContent() == "    x")
    }

    @Test func commandSSaves() {
        let editor = makeEditor()
        var saves = 0
        editor.onSave = { saves += 1 }
        let textView = editor.editorTextView
        let command = textView.keyCommands?.first { $0.input == "s" && $0.modifierFlags == .command }
        #expect(command != nil)
        #expect(command?.wantsPriorityOverSystemBehavior == true)
        textView.saveDocument(nil)
        #expect(saves == 1)
    }

    @Test func readOnlyFilesCannotBeEdited() {
        let editor = makeEditor("locked")
        editor.setEditable(false)
        #expect(!editor.editorTextView.isEditable)
        #expect(editor.editorTextView.isSelectable)
        editor.setEditable(true)
        #expect(editor.editorTextView.isEditable)
    }

    @Test func replacingTheTextClearsUndo() {
        let editor = makeEditor("one")
        let textView = editor.editorTextView
        textView.undoManager?.registerUndo(withTarget: textView) { _ in }
        editor.setContent("two")
        #expect(textView.undoManager?.canUndo != true)
    }

    // MARK: - Highlighting

    @Test func highlightsSwiftKeywordsThroughNeon() async throws {
        let editor = makeEditor("let value = 1\n", language: .swift)
        let textView = editor.editorTextView
        let layoutManager = try #require(textView.textLayoutManager)
        let start = layoutManager.documentRange.location

        var keywordColor: UIColor?
        for _ in 0..<100 where keywordColor == nil {
            layoutManager.enumerateRenderingAttributes(from: start, reverse: false) { _, attrs, _ in
                keywordColor = attrs[.foregroundColor] as? UIColor
                return false
            }
            if keywordColor == nil { try await Task.sleep(for: .milliseconds(20)) }
        }
        #expect(keywordColor == EditorTheme.keyword)
    }

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

    @Test func gutterSitsBesideTheText() {
        let editor = makeEditor("a\nb")
        let gutter = editor.subviews.first { $0 is LineNumberRulerView }
        #expect(gutter?.frame.width == 40)
        #expect(editor.editorTextView.frame.minX == 40)
    }

    // MARK: - Markdown preview

    @Test func remotePreviewsNeverUseFileURLs() {
        let remote = MarkdownPreviewView.baseURL(directory: "/srv/docs", isRemote: true)
        #expect(remote?.absoluteString == "purepoint-remote://files/srv/docs/")
        let local = MarkdownPreviewView.baseURL(directory: "/srv/docs", isRemote: false)
        #expect(local?.isFileURL == true)
        #expect(MarkdownPreviewView.baseURL(directory: nil, isRemote: true) == nil)
    }

    @Test func linksOpenExternallyOrAsFilesBelowTheDocument() throws {
        let web = try #require(URL(string: "https://example.com"))
        #expect(MarkdownPreviewView.linkAction(for: web, baseDirectory: "/srv/docs") == .openExternally(web))
        let mail = try #require(URL(string: "mailto:a@example.com"))
        #expect(MarkdownPreviewView.linkAction(for: mail, baseDirectory: nil) == .openExternally(mail))

        let sibling = try #require(URL(string: "purepoint-remote://files/srv/docs/guide.md"))
        #expect(MarkdownPreviewView.linkAction(for: sibling, baseDirectory: "/srv/docs") == .openFile("/srv/docs/guide.md"))
        let escape = try #require(URL(string: "purepoint-remote://files/srv/docs/../secret.md"))
        #expect(MarkdownPreviewView.linkAction(for: escape, baseDirectory: "/srv/docs") == .ignore)
        let script = try #require(URL(string: "javascript:alert(1)"))
        #expect(MarkdownPreviewView.linkAction(for: script, baseDirectory: "/srv/docs") == .ignore)
    }
}
#endif
