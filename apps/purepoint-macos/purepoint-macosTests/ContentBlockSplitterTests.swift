import Foundation
import Testing
@testable import PurePoint

struct ContentBlockSplitterTests {

    @Test func givenPlainTextShouldReturnSingleTextBlock() {
        let blocks = ContentBlockSplitter.split("Hello, world!")
        #expect(blocks.count == 1)
        guard case .text(_, let text) = blocks[0] else {
            Issue.record("Expected .text block")
            return
        }
        #expect(text == "Hello, world!")
    }

    @Test func givenSingleCodeFenceShouldReturnTextAndCodeBlock() {
        let input = """
            Here is some code:

            ```swift
            let x = 42
            ```

            And some more text.
            """

        let blocks = ContentBlockSplitter.split(input)
        #expect(blocks.count == 3)

        guard case .text(_, let before) = blocks[0] else {
            Issue.record("Expected .text block at 0")
            return
        }
        #expect(before.contains("Here is some code:"))

        guard case .codeBlock(_, let lang, let code) = blocks[1] else {
            Issue.record("Expected .codeBlock at 1")
            return
        }
        #expect(lang == "swift")
        #expect(code == "let x = 42")

        guard case .text(_, let after) = blocks[2] else {
            Issue.record("Expected .text block at 2")
            return
        }
        #expect(after.contains("And some more text."))
    }

    @Test func givenMultipleCodeFencesShouldSplitCorrectly() {
        let input = """
            First:
            ```python
            print("a")
            ```
            Middle text.
            ```rust
            fn main() {}
            ```
            End.
            """

        let blocks = ContentBlockSplitter.split(input)
        #expect(blocks.count == 5)

        guard case .text = blocks[0] else { Issue.record("Expected text at 0"); return }
        guard case .codeBlock(_, let lang1, _) = blocks[1] else { Issue.record("Expected code at 1"); return }
        #expect(lang1 == "python")
        guard case .text = blocks[2] else { Issue.record("Expected text at 2"); return }
        guard case .codeBlock(_, let lang2, _) = blocks[3] else { Issue.record("Expected code at 3"); return }
        #expect(lang2 == "rust")
        guard case .text = blocks[4] else { Issue.record("Expected text at 4"); return }
    }

    @Test func givenCodeFenceWithLanguageShouldCaptureLanguage() {
        let input = """
            ```typescript
            const x: number = 1;
            ```
            """

        let blocks = ContentBlockSplitter.split(input)
        guard
            case .codeBlock(_, let lang, _) = blocks.first(where: {
                if case .codeBlock = $0 { return true }
                return false
            })
        else {
            Issue.record("Expected .codeBlock")
            return
        }
        #expect(lang == "typescript")
    }

    @Test func givenCodeFenceWithoutLanguageShouldDefaultToNil() {
        let input = """
            ```
            plain code
            ```
            """

        let blocks = ContentBlockSplitter.split(input)
        guard
            case .codeBlock(_, let lang, let code) = blocks.first(where: {
                if case .codeBlock = $0 { return true }
                return false
            })
        else {
            Issue.record("Expected .codeBlock")
            return
        }
        #expect(lang == nil)
        #expect(code == "plain code")
    }

    @Test func givenUnclosedCodeFenceShouldTreatAsText() {
        let input = """
            Some text
            ```python
            unclosed code block
            """

        let blocks = ContentBlockSplitter.split(input)
        #expect(blocks.count == 1)
        guard case .text(_, let text) = blocks[0] else {
            Issue.record("Expected .text block")
            return
        }
        #expect(text.contains("unclosed code block"))
    }

    @Test func givenOnlyCodeFenceShouldReturnSingleCodeBlock() {
        let input = """
            ```js
            console.log("hi")
            ```
            """

        let blocks = ContentBlockSplitter.split(input)
        let codeBlocks = blocks.filter {
            if case .codeBlock = $0 { return true }
            return false
        }
        #expect(codeBlocks.count == 1)
        guard case .codeBlock(_, let lang, let code) = codeBlocks[0] else {
            Issue.record("Expected .codeBlock")
            return
        }
        #expect(lang == "js")
        #expect(code == "console.log(\"hi\")")
    }

    @Test func givenEmptyStringShouldReturnEmptyArray() {
        let blocks = ContentBlockSplitter.split("")
        #expect(blocks.isEmpty)
    }
}
