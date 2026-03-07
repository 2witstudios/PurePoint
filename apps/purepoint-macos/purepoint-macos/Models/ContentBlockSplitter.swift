import Foundation

enum ContentBlockSplitter {
    private static func isClosingFence(_ line: String) -> Bool {
        let trimmed = line.trimmingCharacters(in: .whitespaces)
        guard trimmed.hasPrefix("```") else { return false }
        let remainder = trimmed.dropFirst(3)
        return remainder.isEmpty || remainder.allSatisfy(\.isWhitespace)
    }

    /// Split markdown text into content blocks. Uses stable index-based IDs
    /// so SwiftUI can diff efficiently across repeated calls.
    /// Pass `startIndex` to avoid ID collisions when appending multiple split results.
    static func split(_ text: String, startIndex: Int = 0) -> [ContentBlock] {
        guard !text.isEmpty else { return [] }

        let lines = text.split(separator: "\n", omittingEmptySubsequences: false).map(String.init)
        var blocks: [ContentBlock] = []
        var currentText: [String] = []
        var codeLines: [String] = []
        var codeLanguage: String?
        var openingFenceLine: String?
        var savedText: [String] = []
        var inCodeBlock = false
        var blockIndex = startIndex

        func flushText() {
            let joined = currentText.joined(separator: "\n").trimmingCharacters(in: .whitespacesAndNewlines)
            if !joined.isEmpty {
                blocks.append(.text(id: "cb-\(blockIndex)", text: joined))
                blockIndex += 1
            }
            currentText = []
        }

        for line in lines {
            if !inCodeBlock {
                if line.hasPrefix("```") {
                    // Optimistically enter code block — save currentText in case fence is unclosed
                    savedText = currentText
                    currentText = []
                    let langPart = String(line.dropFirst(3)).trimmingCharacters(in: .whitespaces)
                    codeLanguage = langPart.isEmpty ? nil : langPart
                    openingFenceLine = line
                    inCodeBlock = true
                    codeLines = []
                } else {
                    currentText.append(line)
                }
            } else {
                if isClosingFence(line) {
                    // Code block confirmed — flush any preceding text
                    currentText = savedText
                    flushText()
                    savedText = []
                    let code = codeLines.joined(separator: "\n")
                    blocks.append(.codeBlock(id: "cb-\(blockIndex)", language: codeLanguage, code: code))
                    blockIndex += 1
                    inCodeBlock = false
                    codeLines = []
                    codeLanguage = nil
                    openingFenceLine = nil
                } else {
                    codeLines.append(line)
                }
            }
        }

        // EOF while in code block: the fence had no closing — treat as plain text
        if inCodeBlock {
            currentText = savedText
            if let fence = openingFenceLine {
                currentText.append(fence)
            }
            currentText.append(contentsOf: codeLines)
        }

        flushText()
        return blocks
    }
}
