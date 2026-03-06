import Foundation

enum ContentBlockSplitter {
    static func split(_ text: String) -> [ContentBlock] {
        guard !text.isEmpty else { return [] }

        let lines = text.split(separator: "\n", omittingEmptySubsequences: false).map(String.init)
        var blocks: [ContentBlock] = []
        var currentText: [String] = []
        var codeLines: [String] = []
        var codeLanguage: String?
        var inCodeBlock = false
        var i = 0

        while i < lines.count {
            let line = lines[i]

            if !inCodeBlock {
                if line.hasPrefix("```") {
                    // Look ahead for a closing fence
                    var hasClosing = false
                    for j in (i + 1)..<lines.count {
                        if lines[j].trimmingCharacters(in: .whitespaces) == "```" {
                            hasClosing = true
                            break
                        }
                    }

                    if hasClosing {
                        let joined = currentText.joined(separator: "\n").trimmingCharacters(in: .whitespacesAndNewlines)
                        if !joined.isEmpty {
                            blocks.append(.text(id: UUID().uuidString, text: joined))
                        }
                        currentText = []

                        let langPart = String(line.dropFirst(3)).trimmingCharacters(in: .whitespaces)
                        codeLanguage = langPart.isEmpty ? nil : langPart
                        inCodeBlock = true
                        codeLines = []
                    } else {
                        currentText.append(line)
                    }
                } else {
                    currentText.append(line)
                }
            } else {
                if line.trimmingCharacters(in: .whitespaces) == "```" {
                    let code = codeLines.joined(separator: "\n")
                    blocks.append(.codeBlock(id: UUID().uuidString, language: codeLanguage, code: code))
                    inCodeBlock = false
                    codeLines = []
                    codeLanguage = nil
                } else {
                    codeLines.append(line)
                }
            }

            i += 1
        }

        let joined = currentText.joined(separator: "\n").trimmingCharacters(in: .whitespacesAndNewlines)
        if !joined.isEmpty {
            blocks.append(.text(id: UUID().uuidString, text: joined))
        }

        return blocks
    }
}
