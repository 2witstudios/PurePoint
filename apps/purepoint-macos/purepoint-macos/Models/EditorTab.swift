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
    case tsx
    case python
    case markdown
    case json
    case yaml
    case toml
    case html
    case css
    case shell
    case c
    case cpp
    case go
    case ruby
    case php
    case java
    case csharp
    case scala
    case haskell
    case lua
    case xml
    case plaintext

    static func detect(from filename: String) -> EditorLanguage {
        let name = (filename as NSString).lastPathComponent.lowercased()

        if let match = knownFilenames[name] { return match }

        let ext = (filename as NSString).pathExtension.lowercased()
        switch ext {
        case "swift": return .swift
        case "rs": return .rust
        case "js", "jsx", "mjs", "cjs": return .javascript
        case "ts", "mts", "cts": return .typescript
        case "tsx": return .tsx
        case "py", "pyi": return .python
        case "md", "markdown", "mdx": return .markdown
        case "json", "jsonc", "json5": return .json
        case "yml", "yaml": return .yaml
        case "toml": return .toml
        case "html", "htm": return .html
        case "css", "scss", "sass", "less": return .css
        case "sh", "bash", "zsh", "fish": return .shell
        case "c", "h": return .c
        case "cpp", "cc", "cxx", "hpp", "hxx", "hh": return .cpp
        case "go": return .go
        case "rb", "rake", "gemspec": return .ruby
        case "php": return .php
        case "java": return .java
        case "cs": return .csharp
        case "scala", "sc": return .scala
        case "hs", "lhs": return .haskell
        case "lua": return .lua
        case "xml", "xsl", "xslt", "svg", "plist": return .xml
        case "graphql", "gql": return .javascript
        case "dockerfile": return .shell
        default: return .plaintext
        }
    }

    private static let knownFilenames: [String: EditorLanguage] = [
        // Lock files
        "cargo.lock": .toml,
        "package.resolved": .json,
        "composer.lock": .json,
        // Ruby conventions
        "gemfile": .ruby,
        "rakefile": .ruby,
        "podfile": .ruby,
        "vagrantfile": .ruby,
        "fastfile": .ruby,
        "brewfile": .ruby,
        "dangerfile": .ruby,
        "guardfile": .ruby,
        // Build/task files
        "makefile": .shell,
        "gnumakefile": .shell,
        "dockerfile": .shell,
        "justfile": .shell,
        "procfile": .shell,
        // Ignore files
        ".gitignore": .shell,
        ".dockerignore": .shell,
        ".npmignore": .shell,
        ".hgignore": .shell,
        // Config dotfiles
        ".editorconfig": .plaintext,
        ".gitconfig": .plaintext,
        ".gitmodules": .plaintext,
        // Shell dotfiles
        ".zshrc": .shell,
        ".bashrc": .shell,
        ".bash_profile": .shell,
        ".profile": .shell,
        ".zprofile": .shell,
        ".zshenv": .shell,
        ".bash_aliases": .shell,
        ".bash_logout": .shell,
    ]

    var icon: String {
        switch self {
        case .swift: return "swift"
        case .rust: return "gearshape.2"
        case .javascript, .typescript, .tsx: return "curlybraces"
        case .python: return "chevron.left.forwardslash.chevron.right"
        case .markdown: return "doc.text"
        case .json, .yaml, .toml: return "doc.badge.gearshape"
        case .html, .xml: return "globe"
        case .css: return "paintbrush"
        case .shell: return "terminal"
        case .c, .cpp: return "c.square"
        case .go: return "arrow.right.arrow.left"
        case .ruby: return "diamond"
        case .php: return "server.rack"
        case .java: return "cup.and.saucer"
        case .csharp: return "number"
        case .scala: return "s.square"
        case .haskell: return "function"
        case .lua: return "moon"
        case .plaintext: return "doc"
        }
    }
}
