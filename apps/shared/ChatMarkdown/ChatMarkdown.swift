import Foundation

/// Presentation blocks shared by phone and desktop. Parsing is independent of SwiftUI
/// and tolerates unfinished fences while a reply streams. Raw HTML remains text.
indirect enum ChatMarkdownBlock: Equatable, Sendable {
    case paragraph(String)
    case heading(level: Int, text: String)
    case listItem(depth: Int, marker: String, checked: Bool?, text: String)
    case listContinuation(depth: Int, blocks: [ChatMarkdownBlock])
    case quote([ChatMarkdownBlock])
    case code(language: String, text: String)
    case table(headers: [String], alignments: [ChatTableAlignment], rows: [[String]])
    case rule
    case image(alt: String, url: URL)
}
enum ChatTableAlignment: Sendable { case leading, center, trailing }

nonisolated struct ChatMarkdownDocument: Sendable {
    let blocks: [ChatMarkdownBlock]
    let inlineContent: [String: AttributedString]
}

nonisolated enum ChatMarkdown {
    static func document(_ source: String) -> ChatMarkdownDocument {
        let blocks = parse(source)
        var inlineContent: [String: AttributedString] = [:]
        func prepare(_ text: String) {
            guard !Task.isCancelled, inlineContent[text] == nil else { return }
            var content = (try? AttributedString(markdown: text, options: .init(interpretedSyntax: .inlineOnlyPreservingWhitespace))) ?? AttributedString(text)
            for run in content.runs {
                if let url = run[AttributeScopes.FoundationAttributes.LinkAttribute.self], !["http", "https", "mailto"].contains(url.scheme?.lowercased() ?? "") {
                    content[run.range][AttributeScopes.FoundationAttributes.LinkAttribute.self] = nil
                }
            }
            inlineContent[text] = content
        }
        func visit(_ blocks: [ChatMarkdownBlock]) {
            for block in blocks {
                guard !Task.isCancelled else { return }
                switch block {
                case .paragraph(let text), .heading(_, let text), .listItem(_, _, _, let text): prepare(text)
                case .quote(let nested), .listContinuation(_, let nested): visit(nested)
                case .table(let headers, _, let rows):
                    for cell in headers { prepare(cell) }
                    for row in rows { for cell in row { prepare(cell) } }
                case .code, .rule, .image: break
                }
            }
        }
        visit(blocks)
        return ChatMarkdownDocument(blocks: blocks, inlineContent: inlineContent)
    }
    static func parse(_ source: String) -> [ChatMarkdownBlock] {
        parse(source, nesting: 0)
    }
    private static func parse(_ source: String, nesting: Int) -> [ChatMarkdownBlock] {
        guard !Task.isCancelled else { return [] }
        // Replies are untrusted content. Keep parser and SwiftUI recursion bounded.
        guard nesting < 32 else { return source.isEmpty ? [] : [.paragraph(source)] }
        let lines = source.replacingOccurrences(of: "\r\n", with: "\n").replacingOccurrences(of: "\r", with: "\n").components(separatedBy: "\n")
        var blocks: [ChatMarkdownBlock] = []
        var paragraph: [String] = []
        var i = 0
        func flush() {
            if !paragraph.isEmpty { blocks.append(.paragraph(paragraph.joined(separator: "\n"))); paragraph = [] }
        }
        while i < lines.count {
            guard !Task.isCancelled else { return [] }
            let line = lines[i], trimmed = line.trimmingCharacters(in: .whitespaces)
            if trimmed.isEmpty { flush(); i += 1; continue }
            if let fence = fence(trimmed) {
                flush(); i += 1
                var code: [String] = []
                while i < lines.count {
                    guard !Task.isCancelled else { return [] }
                    let candidate = lines[i].trimmingCharacters(in: .whitespaces)
                    let run = candidate.prefix(while: { $0 == fence.character })
                    if run.count >= fence.count && candidate.dropFirst(run.count).trimmingCharacters(in: .whitespaces).isEmpty { i += 1; break }
                    code.append(lines[i]); i += 1
                }
                blocks.append(.code(language: fence.language, text: code.joined(separator: "\n"))); continue
            }
            let hashes = trimmed.prefix(while: { $0 == "#" }).count
            if (1...6).contains(hashes), trimmed.dropFirst(hashes).first == " " {
                flush(); blocks.append(.heading(level: hashes, text: String(trimmed.dropFirst(hashes + 1)))); i += 1; continue
            }
            if isRule(trimmed) { flush(); blocks.append(.rule); i += 1; continue }
            if trimmed.hasPrefix(">") {
                flush(); var quoted: [String] = []
                while i < lines.count {
                    guard !Task.isCancelled else { return [] }
                    let current = lines[i].trimmingCharacters(in: .whitespaces)
                    guard current.hasPrefix(">") else { break }
                    var content = current.dropFirst()
                    if content.first == " " { content = content.dropFirst() }
                    quoted.append(String(content)); i += 1
                }
                blocks.append(.quote(parse(quoted.joined(separator: "\n"), nesting: nesting + 1))); continue
            }
            if i + 1 < lines.count, trimmed.contains("|"), let alignments = tableDelimiter(lines[i + 1]), alignments.count == cells(trimmed).count {
                flush(); let headers = cells(trimmed); i += 2
                var rows: [[String]] = []
                while i < lines.count, lines[i].contains("|"), !lines[i].trimmingCharacters(in: .whitespaces).isEmpty {
                    guard !Task.isCancelled else { return [] }
                    let row = cells(lines[i]); rows.append(Array((row + Array(repeating: "", count: headers.count)).prefix(headers.count))); i += 1
                }
                blocks.append(.table(headers: headers, alignments: alignments, rows: rows)); continue
            }
            if let image = image(trimmed) {
                flush(); blocks.append(.image(alt: image.alt, url: image.url)); i += 1; continue
            }
            if let item = listItem(line) {
                flush(); i += 1
                var continuation: [String] = []
                var activeFence: (character: Character, count: Int, language: String)?
                while i < lines.count {
                    guard !Task.isCancelled else { return [] }
                    let next = lines[i], nextTrimmed = next.trimmingCharacters(in: .whitespaces)
                    if let current = activeFence {
                        let run = nextTrimmed.prefix(while: { $0 == current.character })
                        if run.count >= current.count && nextTrimmed.dropFirst(run.count).trimmingCharacters(in: .whitespaces).isEmpty { activeFence = nil }
                    } else {
                        if nextTrimmed.isEmpty {
                            var lookahead = i + 1
                            while lookahead < lines.count, lines[lookahead].trimmingCharacters(in: .whitespaces).isEmpty { lookahead += 1 }
                            guard lookahead < lines.count, indentation(lines[lookahead]) > item.indent, listItem(lines[lookahead]) == nil else { break }
                        } else {
                            guard indentation(next) > item.indent, listItem(next) == nil else { break }
                            activeFence = fence(nextTrimmed)
                        }
                    }
                    continuation.append(next); i += 1
                }
                let depth = min(item.indent / 2, 8)
                let margin = continuation.first(where: { !$0.trimmingCharacters(in: .whitespaces).isEmpty }).map(indentation) ?? 0
                let body = continuation.map { removingIndent($0, columns: margin) }.joined(separator: "\n")
                let contents = parse(body, nesting: nesting + 1)
                if case .paragraph(let continued) = contents.first, contents.count == 1 {
                    blocks.append(.listItem(depth: depth, marker: item.marker, checked: item.checked, text: item.text + "\n" + continued))
                } else {
                    blocks.append(.listItem(depth: depth, marker: item.marker, checked: item.checked, text: item.text))
                    if !contents.isEmpty { blocks.append(.listContinuation(depth: min(depth + 1, 8), blocks: contents)) }
                }
                continue
            }
            // Setext headings, common in saved assistant replies.
            if i + 1 < lines.count {
                let underline = lines[i + 1].trimmingCharacters(in: .whitespaces)
                if underline.count >= 3, underline.allSatisfy({ $0 == "=" }) || (underline.count >= 3 && underline.allSatisfy({ $0 == "-" })) {
                    flush(); blocks.append(.heading(level: underline.first == "=" ? 1 : 2, text: trimmed)); i += 2; continue
                }
            }
            paragraph.append(line); i += 1
        }
        flush(); return blocks
    }
    private static func image(_ line: String) -> (alt: String, url: URL)? {
        guard let regex = try? NSRegularExpression(pattern: "^!\\[([^\\]]*)\\]\\((https?://[^\\s]+)\\)$"),
              let match = regex.firstMatch(in: line, range: NSRange(line.startIndex..., in: line)),
              let altRange = Range(match.range(at: 1), in: line), let urlRange = Range(match.range(at: 2), in: line),
              let url = URL(string: String(line[urlRange])) else { return nil }
        return (String(line[altRange]), url)
    }
    private static func fence(_ line: String) -> (character: Character, count: Int, language: String)? {
        guard let first = line.first, first == "`" || first == "~" else { return nil }
        let count = line.prefix(while: { $0 == first }).count
        guard count >= 3 else { return nil }
        return (first, count, String(line.dropFirst(count)).trimmingCharacters(in: .whitespaces))
    }
    private static func isRule(_ line: String) -> Bool {
        let compact = line.filter { !$0.isWhitespace }
        return compact.count >= 3 && compact.first.map { "-*_".contains($0) && compact.allSatisfy { $0 == compact.first } } == true
    }
    private static func indentation(_ line: String) -> Int { line.prefix(while: { $0 == " " || $0 == "\t" }).reduce(0) { $0 + ($1 == "\t" ? 4 : 1) } }
    private static func removingIndent(_ line: String, columns: Int) -> String {
        var remaining = line[...], removed = 0
        while removed < columns, let first = remaining.first, first == " " || first == "\t" {
            removed += first == "\t" ? 4 : 1; remaining = remaining.dropFirst()
        }
        return String(remaining)
    }
    private static func listItem(_ line: String) -> (indent: Int, marker: String, checked: Bool?, text: String)? {
        let t = line.trimmingCharacters(in: .whitespaces)
        var marker = "•", content: String
        if ["- ", "+ ", "* "].contains(where: { t.hasPrefix($0) }) { content = String(t.dropFirst(2)) }
        else {
            let digits = t.prefix(while: { $0.isNumber })
            let rest = t.dropFirst(digits.count)
            guard !digits.isEmpty, digits.count <= 9, rest.hasPrefix(". ") || rest.hasPrefix(") ") else { return nil }
            marker = String(digits) + "."; content = String(rest.dropFirst(2))
        }
        var checked: Bool?
        if content.hasPrefix("[ ] ") || content.lowercased().hasPrefix("[x] ") { checked = content.lowercased().hasPrefix("[x]"); content = String(content.dropFirst(4)) }
        return (indentation(line), marker, checked, content)
    }
    /// Split pipes outside code spans; escaped pipes belong to their cell.
    static func cells(_ source: String) -> [String] {
        var line = source.trimmingCharacters(in: .whitespaces)
        if line.hasPrefix("|") { line.removeFirst() }
        if line.hasSuffix("|"), !line.hasSuffix("\\|") { line.removeLast() }
        let characters = Array(line)
        var result: [String] = [], cell = "", escaped = false, delimiter = 0, index = 0
        func hasClosingRun(_ length: Int, after start: Int) -> Bool {
            var cursor = start
            while cursor < characters.count {
                guard !Task.isCancelled else { return false }
                if characters[cursor] == "`" {
                    var end = cursor
                    while end < characters.count, characters[end] == "`" { end += 1 }
                    if end - cursor == length { return true }
                    cursor = end
                } else { cursor += 1 }
            }
            return false
        }
        while index < characters.count {
            guard !Task.isCancelled else { return [] }
            let character = characters[index]
            index += 1
            if escaped { if character != "|" { cell.append("\\") }; cell.append(character); escaped = false; continue }
            if character == "\\", delimiter == 0 { escaped = true; continue }
            if character == "`" {
                let start = index - 1
                while index < characters.count, characters[index] == "`" { index += 1 }
                let length = index - start
                if delimiter == length { delimiter = 0 }
                else if delimiter == 0, hasClosingRun(length, after: index) { delimiter = length }
                cell += String(repeating: "`", count: length)
                continue
            }
            if character == "|" && delimiter == 0 { result.append(cell.trimmingCharacters(in: .whitespaces)); cell = "" }
            else { cell.append(character) }
        }
        if escaped { cell.append("\\") }
        result.append(cell.trimmingCharacters(in: .whitespaces)); return result
    }
    private static func tableDelimiter(_ source: String) -> [ChatTableAlignment]? {
        let parts = cells(source)
        guard !parts.isEmpty, parts.allSatisfy({ $0.filter { $0 == "-" }.count >= 3 && $0.allSatisfy { "-:".contains($0) } }) else { return nil }
        return parts.map { $0.hasSuffix(":") ? ($0.hasPrefix(":") ? .center : .trailing) : .leading }
    }
}
