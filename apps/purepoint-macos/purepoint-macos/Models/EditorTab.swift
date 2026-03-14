import Foundation

struct EditorTab: Identifiable {
    let id: String  // absolute path
    let name: String
    var content: String
    var isDirty: Bool
    var lastModified: Date?
    let isBinary: Bool
    let language: EditorLanguage
}

enum EditorLanguage: String, CaseIterable {
    case swift
    case rust
    case javascript
    case typescript
    case python
    case markdown
    case json
    case yaml
    case toml
    case html
    case css
    case shell
    case plaintext

    static func detect(from filename: String) -> EditorLanguage {
        let ext = (filename as NSString).pathExtension.lowercased()
        switch ext {
        case "swift": return .swift
        case "rs": return .rust
        case "js", "jsx", "mjs", "cjs": return .javascript
        case "ts", "tsx", "mts", "cts": return .typescript
        case "py", "pyi": return .python
        case "md", "markdown": return .markdown
        case "json": return .json
        case "yml", "yaml": return .yaml
        case "toml": return .toml
        case "html", "htm": return .html
        case "css", "scss", "sass", "less": return .css
        case "sh", "bash", "zsh", "fish": return .shell
        default: return .plaintext
        }
    }

    var icon: String {
        switch self {
        case .swift: return "swift"
        case .rust: return "gearshape.2"
        case .javascript, .typescript: return "curlybraces"
        case .python: return "chevron.left.forwardslash.chevron.right"
        case .markdown: return "doc.text"
        case .json, .yaml, .toml: return "doc.badge.gearshape"
        case .html: return "globe"
        case .css: return "paintbrush"
        case .shell: return "terminal"
        case .plaintext: return "doc"
        }
    }
}
