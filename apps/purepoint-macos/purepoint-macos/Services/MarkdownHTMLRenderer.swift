import Foundation

/// Small, dependency-free Markdown → HTML converter for the file pane's preview.
/// Covers the constructs docs actually use (headings, fences, lists, quotes, rules, tables,
/// inline code/emphasis/links/images). All text is HTML-escaped before inline markup is applied,
/// so file content can never inject markup or script.
nonisolated enum MarkdownHTMLRenderer {

    static func render(_ markdown: String) -> String {
        let lines = markdown.replacingOccurrences(of: "\r\n", with: "\n").components(separatedBy: "\n")
        var html = ""
        var paragraph: [String] = []
        var i = 0

        func flushParagraph() {
            guard !paragraph.isEmpty else { return }
            html += "<p>\(inline(paragraph.joined(separator: " ")))</p>\n"
            paragraph.removeAll()
        }

        while i < lines.count {
            let line = lines[i]
            let trimmed = line.trimmingCharacters(in: .whitespaces)

            // Fenced code block
            if trimmed.hasPrefix("```") || trimmed.hasPrefix("~~~") {
                flushParagraph()
                let fence = String(trimmed.prefix(3))
                let lang = trimmed.dropFirst(3).trimmingCharacters(in: .whitespaces)
                var code: [String] = []
                i += 1
                while i < lines.count, !lines[i].trimmingCharacters(in: .whitespaces).hasPrefix(fence) {
                    code.append(lines[i])
                    i += 1
                }
                i += 1  // closing fence (or EOF)
                let cls = lang.isEmpty ? "" : " class=\"language-\(escape(lang))\""
                html += "<pre><code\(cls)>\(escape(code.joined(separator: "\n")))</code></pre>\n"
                continue
            }

            if trimmed.isEmpty {
                flushParagraph()
                i += 1
                continue
            }

            // Heading
            if let level = headingLevel(trimmed) {
                flushParagraph()
                let text = trimmed.dropFirst(level).trimmingCharacters(in: .whitespaces)
                html += "<h\(level)>\(inline(text))</h\(level)>\n"
                i += 1
                continue
            }

            // Horizontal rule
            if isRule(trimmed) {
                flushParagraph()
                html += "<hr>\n"
                i += 1
                continue
            }

            // Blockquote
            if trimmed.hasPrefix(">") {
                flushParagraph()
                var quoted: [String] = []
                while i < lines.count, lines[i].trimmingCharacters(in: .whitespaces).hasPrefix(">") {
                    var t = lines[i].trimmingCharacters(in: .whitespaces).dropFirst()
                    if t.hasPrefix(" ") { t = t.dropFirst() }
                    quoted.append(String(t))
                    i += 1
                }
                html += "<blockquote>\(render(quoted.joined(separator: "\n")))</blockquote>\n"
                continue
            }

            // Table: header row followed by a delimiter row
            if trimmed.contains("|"), i + 1 < lines.count, isTableDelimiter(lines[i + 1]) {
                flushParagraph()
                let header = cells(trimmed)
                i += 2
                var rows: [[String]] = []
                while i < lines.count, lines[i].contains("|"),
                    !lines[i].trimmingCharacters(in: .whitespaces).isEmpty
                {
                    rows.append(cells(lines[i]))
                    i += 1
                }
                html += "<table><thead><tr>" + header.map { "<th>\(inline($0))</th>" }.joined()
                html += "</tr></thead><tbody>"
                for row in rows {
                    html += "<tr>" + row.map { "<td>\(inline($0))</td>" }.joined() + "</tr>"
                }
                html += "</tbody></table>\n"
                continue
            }

            // Lists (flat; indentation inside a list continues the previous item)
            if let marker = listMarker(line) {
                flushParagraph()
                let ordered = marker.ordered
                var items: [String] = []
                while i < lines.count, let m = listMarker(lines[i]), m.ordered == ordered {
                    items.append(m.text)
                    i += 1
                }
                let tag = ordered ? "ol" : "ul"
                html += "<\(tag)>" + items.map { "<li>\(inline($0))</li>" }.joined() + "</\(tag)>\n"
                continue
            }

            paragraph.append(trimmed)
            i += 1
        }
        flushParagraph()
        return html
    }

    /// A full page around the rendered body, themed for light and dark.
    static func page(body: String) -> String {
        """
        <!doctype html><html><head><meta charset="utf-8">
        <meta name="color-scheme" content="light dark">
        <style>
        :root { color-scheme: light dark; }
        body { font: 14px/1.6 -apple-system, BlinkMacSystemFont, sans-serif; margin: 0; padding: 16px 20px;
               color: CanvasText; background: Canvas; overflow-wrap: anywhere; }
        h1,h2,h3,h4,h5,h6 { line-height: 1.25; margin: 1.2em 0 .5em; }
        h1 { font-size: 1.7em; border-bottom: 1px solid rgba(128,128,128,.3); padding-bottom: .3em; }
        h2 { font-size: 1.4em; border-bottom: 1px solid rgba(128,128,128,.2); padding-bottom: .2em; }
        code { font: 12.5px ui-monospace, Menlo, monospace; background: rgba(128,128,128,.18);
               padding: .1em .35em; border-radius: 4px; }
        pre { background: rgba(128,128,128,.14); padding: 12px; border-radius: 6px; overflow-x: auto; }
        pre code { background: none; padding: 0; }
        blockquote { margin: 0 0 1em; padding: 0 1em; border-left: 3px solid rgba(128,128,128,.4); opacity: .85; }
        table { border-collapse: collapse; } th, td { border: 1px solid rgba(128,128,128,.35); padding: 4px 10px; }
        a { color: LinkText; } img { max-width: 100%; } hr { border: 0; border-top: 1px solid rgba(128,128,128,.3); }
        </style></head><body>\(body)</body></html>
        """
    }

    // MARK: - Helpers

    static func escape(_ text: String) -> String {
        text.replacingOccurrences(of: "&", with: "&amp;")
            .replacingOccurrences(of: "<", with: "&lt;")
            .replacingOccurrences(of: ">", with: "&gt;")
            .replacingOccurrences(of: "\"", with: "&quot;")
    }

    private static func headingLevel(_ line: String) -> Int? {
        let hashes = line.prefix(while: { $0 == "#" }).count
        guard (1...6).contains(hashes), line.dropFirst(hashes).first == " " else { return nil }
        return hashes
    }

    private static func isRule(_ line: String) -> Bool {
        let compact = line.replacingOccurrences(of: " ", with: "")
        guard compact.count >= 3, let first = compact.first, "-*_".contains(first) else { return false }
        return compact.allSatisfy { $0 == first }
    }

    private static func isTableDelimiter(_ line: String) -> Bool {
        let t = line.trimmingCharacters(in: .whitespaces)
        guard t.contains("-"), t.contains("|") else { return false }
        return t.allSatisfy { "|-: ".contains($0) }
    }

    private static func cells(_ line: String) -> [String] {
        var t = line.trimmingCharacters(in: .whitespaces)
        if t.hasPrefix("|") { t.removeFirst() }
        if t.hasSuffix("|") { t.removeLast() }
        return t.components(separatedBy: "|").map { $0.trimmingCharacters(in: .whitespaces) }
    }

    private static func listMarker(_ line: String) -> (ordered: Bool, text: String)? {
        let t = line.trimmingCharacters(in: .whitespaces)
        for bullet in ["- ", "* ", "+ "] where t.hasPrefix(bullet) {
            return (false, String(t.dropFirst(2)))
        }
        let digits = t.prefix(while: \.isNumber)
        if !digits.isEmpty, digits.count <= 9 {
            let rest = t.dropFirst(digits.count)
            if rest.hasPrefix(". ") || rest.hasPrefix(") ") { return (true, String(rest.dropFirst(2))) }
        }
        return nil
    }

    /// Inline markup on already-untrusted text: escape first, then apply patterns.
    static func inline(_ raw: String) -> String {
        var codeSpans: [String] = []
        var text = escape(raw)

        // Pull code spans out first so their contents are not further processed.
        text = replace(text, pattern: "`([^`]+)`") { groups in
            codeSpans.append("<code>\(groups[1])</code>")
            return "\u{0}\(codeSpans.count - 1)\u{0}"
        }
        text = replace(text, pattern: "!\\[([^\\]]*)\\]\\(([^)\\s]+)\\)") { g in
            "<img alt=\"\(g[1])\" src=\"\(safeURL(g[2]))\">"
        }
        text = replace(text, pattern: "\\[([^\\]]+)\\]\\(([^)\\s]+)\\)") { g in
            "<a href=\"\(safeURL(g[2]))\">\(g[1])</a>"
        }
        text = replace(text, pattern: "\\*\\*(.+?)\\*\\*") { "<strong>\($0[1])</strong>" }
        text = replace(text, pattern: "__(.+?)__") { "<strong>\($0[1])</strong>" }
        text = replace(text, pattern: "(?<![\\w*])\\*(?!\\s)(.+?)(?<!\\s)\\*(?![\\w*])") { "<em>\($0[1])</em>" }
        text = replace(text, pattern: "(?<![\\w_])_(?!\\s)(.+?)(?<!\\s)_(?![\\w_])") { "<em>\($0[1])</em>" }
        text = replace(text, pattern: "~~(.+?)~~") { "<del>\($0[1])</del>" }

        for (index, span) in codeSpans.enumerated() {
            text = text.replacingOccurrences(of: "\u{0}\(index)\u{0}", with: span)
        }
        return text
    }

    /// Only http(s), mailto and scheme-less (relative/anchor) URLs survive; anything else
    /// (notably `javascript:`) becomes an inert anchor.
    private static func safeURL(_ url: String) -> String {
        let lower = url.lowercased()
        if lower.hasPrefix("http://") || lower.hasPrefix("https://") || lower.hasPrefix("mailto:") { return url }
        if let colon = lower.firstIndex(of: ":"), !lower[..<colon].contains("/") { return "#" }
        return url
    }

    private static func replace(_ text: String, pattern: String, _ build: ([String]) -> String) -> String {
        guard let regex = try? NSRegularExpression(pattern: pattern) else { return text }
        let ns = text as NSString
        var result = ""
        var last = 0
        for match in regex.matches(in: text, range: NSRange(location: 0, length: ns.length)) {
            result += ns.substring(with: NSRange(location: last, length: match.range.location - last))
            let groups = (0..<match.numberOfRanges).map { idx -> String in
                let r = match.range(at: idx)
                return r.location == NSNotFound ? "" : ns.substring(with: r)
            }
            result += build(groups)
            last = match.range.location + match.range.length
        }
        result += ns.substring(from: last)
        return result
    }
}
