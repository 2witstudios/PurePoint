import AppKit
import Neon
import SwiftTreeSitter
import TreeSitterSwift
import TreeSitterRust
import TreeSitterJavaScript
import TreeSitterTypeScript
import TreeSitterTSX
import TreeSitterPython
import TreeSitterJSON
import TreeSitterBash
import TreeSitterHTML
import TreeSitterCSS
import TreeSitterYAML
import TreeSitterTOML
import TreeSitterMarkdown
import TreeSitterMarkdownInline
import TreeSitterCPP
import TreeSitterGo
import TreeSitterRuby
import TreeSitterPHP
import TreeSitterCSharp
import TreeSitterScala
import TreeSitterHaskell
import TreeSitterLua
import TreeSitterXML

@MainActor
final class SyntaxHighlightManager {
    private weak var textView: NSTextView?
    private var highlighter: TextViewHighlighter?
    private var currentLanguage: EditorLanguage = .plaintext

    private static let maxHighlightSize = 1_000_000
    private static var languageConfigs: [EditorLanguage: LanguageConfiguration] = [:]
    private static let regularFont = NSFont.monospacedSystemFont(ofSize: 13, weight: .regular)
    private static let boldFont = NSFont.monospacedSystemFont(ofSize: 13, weight: .bold)
    private static let italicFont: NSFont = {
        NSFontManager.shared.convert(regularFont, toHaveTrait: .italicFontMask)
    }()

    init(textView: NSTextView) {
        self.textView = textView
    }

    func setLanguage(_ language: EditorLanguage) {
        guard language != currentLanguage else { return }
        currentLanguage = language
        rebuildHighlighter()
    }

    func invalidate() {
        rebuildHighlighter()
    }

    // MARK: - Private

    private func rebuildHighlighter() {
        highlighter = nil

        guard let textView else {
            clearTemporaryAttributes()
            return
        }

        if textView.string.utf8.count > Self.maxHighlightSize {
            clearTemporaryAttributes()
            return
        }

        guard let langConfig = Self.languageConfiguration(for: currentLanguage) else {
            clearTemporaryAttributes()
            return
        }

        do {
            let config = TextViewHighlighter.Configuration(
                languageConfiguration: langConfig,
                attributeProvider: Self.attributeProvider,
                languageProvider: { name in
                    Self.languageConfigurationForInjection(name)
                },
                locationTransformer: { [weak textView] offset in
                    guard let textView, let storage = textView.textStorage else { return nil }
                    let string = storage.string as NSString
                    guard offset <= string.length else { return nil }
                    let head = string.substring(to: offset)
                    let lines = head.components(separatedBy: "\n")
                    let row = lines.count - 1
                    let col = (lines.last ?? "").utf16.count
                    return Point(row: row, column: col)
                }
            )
            highlighter = try TextViewHighlighter(textView: textView, configuration: config)
        } catch {
            clearTemporaryAttributes()
        }
    }

    private func clearTemporaryAttributes() {
        guard let textView, let layoutManager = textView.layoutManager,
            let storage = textView.textStorage, storage.length > 0
        else { return }
        let range = NSRange(location: 0, length: storage.length)
        layoutManager.removeTemporaryAttribute(.foregroundColor, forCharacterRange: range)
        layoutManager.removeTemporaryAttribute(.font, forCharacterRange: range)
    }

    // MARK: - Language Configuration Registry

    private static func languageConfiguration(for language: EditorLanguage) -> LanguageConfiguration? {
        if let cached = languageConfigs[language] { return cached }

        let config: LanguageConfiguration?
        do {
            switch language {
            case .swift:
                config = try LanguageConfiguration(tree_sitter_swift(), name: "Swift")
            case .rust:
                config = try LanguageConfiguration(tree_sitter_rust(), name: "Rust")
            case .javascript:
                config = try LanguageConfiguration(tree_sitter_javascript(), name: "JavaScript")
            case .typescript:
                config = try Self.makeTypeScriptConfig()
            case .tsx:
                config = try Self.makeTSXConfig()
            case .python:
                config = try LanguageConfiguration(tree_sitter_python(), name: "Python")
            case .json:
                config = try LanguageConfiguration(tree_sitter_json(), name: "JSON")
            case .shell:
                config = try LanguageConfiguration(tree_sitter_bash(), name: "Bash")
            case .html:
                config = try LanguageConfiguration(tree_sitter_html(), name: "HTML")
            case .css:
                config = try LanguageConfiguration(tree_sitter_css(), name: "CSS")
            case .yaml:
                config = try LanguageConfiguration(tree_sitter_yaml(), name: "YAML")
            case .toml:
                config = try LanguageConfiguration(tree_sitter_toml(), name: "TOML")
            case .markdown:
                config = try LanguageConfiguration(tree_sitter_markdown(), name: "Markdown")
            case .c:
                return nil  // TODO: Add tree-sitter-c SPM package (SPM resolution conflict)
            case .cpp:
                config = try LanguageConfiguration(
                    tree_sitter_cpp(), name: "C++",
                    bundleName: "TreeSitterCPP_TreeSitterCPP"
                )
            case .go:
                config = try LanguageConfiguration(tree_sitter_go(), name: "Go")
            case .ruby:
                config = try LanguageConfiguration(tree_sitter_ruby(), name: "Ruby")
            case .php:
                config = try LanguageConfiguration(tree_sitter_php(), name: "PHP")
            case .java:
                return nil  // TODO: Add tree-sitter-java SPM package (Xcode conflicts with javascript)
            case .csharp:
                config = try LanguageConfiguration(
                    tree_sitter_c_sharp(), name: "C#",
                    bundleName: "TreeSitterCSharp_TreeSitterCSharp"
                )
            case .scala:
                config = try LanguageConfiguration(tree_sitter_scala(), name: "Scala")
            case .haskell:
                config = try LanguageConfiguration(tree_sitter_haskell(), name: "Haskell")
            case .lua:
                config = try LanguageConfiguration(tree_sitter_lua(), name: "Lua")
            case .xml:
                guard
                    let xmlBundle = Bundle.main.url(
                        forResource: "TreeSitterXML_TreeSitterXML",
                        withExtension: "bundle"
                    )
                else { return nil }
                let xmlQueriesURL = xmlBundle.appendingPathComponent(
                    "Contents/Resources/xml", isDirectory: true
                )
                config = try LanguageConfiguration(
                    tree_sitter_xml(), name: "XML", queriesURL: xmlQueriesURL
                )
            case .plaintext:
                return nil
            }
        } catch {
            return nil
        }

        if let config { languageConfigs[language] = config }
        return config
    }

    // MARK: - Language Injection Provider

    private static func languageConfigurationForInjection(_ name: String) -> LanguageConfiguration? {
        switch name {
        case "javascript": return languageConfiguration(for: .javascript)
        case "css": return languageConfiguration(for: .css)
        case "html": return languageConfiguration(for: .html)
        case "json": return languageConfiguration(for: .json)
        case "bash": return languageConfiguration(for: .shell)
        case "python": return languageConfiguration(for: .python)
        case "typescript": return languageConfiguration(for: .typescript)
        case "yaml": return languageConfiguration(for: .yaml)
        case "toml": return languageConfiguration(for: .toml)
        case "markdown_inline":
            let lang = Language(tree_sitter_markdown_inline())
            return try? LanguageConfiguration(
                lang, name: "MarkdownInline",
                bundleName: "TreeSitterMarkdown_TreeSitterMarkdownInline"
            )
        case "regex", "jsdoc": return nil
        default: return nil
        }
    }

    // MARK: - TypeScript / TSX Query Merging

    private static func makeTypeScriptConfig() throws -> LanguageConfiguration {
        let lang = Language(tree_sitter_typescript())
        let queries = try mergedQueries(
            language: lang,
            baseBundleName: "TreeSitterJavaScript_TreeSitterJavaScript",
            overlayBundleName: "TreeSitterTypeScript_TreeSitterTypeScript"
        )
        return LanguageConfiguration(lang, name: "TypeScript", queries: queries)
    }

    private static func makeTSXConfig() throws -> LanguageConfiguration {
        let lang = Language(tree_sitter_tsx())
        let queries = try mergedQueries(
            language: lang,
            baseBundleName: "TreeSitterJavaScript_TreeSitterJavaScript",
            overlayBundleName: "TreeSitterTypeScript_TreeSitterTypeScript",
            extraBaseHighlights: ["highlights-jsx"]
        )
        return LanguageConfiguration(lang, name: "TSX", queries: queries)
    }

    private static func mergedQueries(
        language: Language,
        baseBundleName: String,
        overlayBundleName: String,
        extraBaseHighlights: [String] = []
    ) throws -> [Query.Definition: Query] {
        guard let baseURL = queriesDirectoryURL(for: baseBundleName),
            let overlayURL = queriesDirectoryURL(for: overlayBundleName)
        else {
            throw MergedQueryError.bundleNotFound
        }

        var queries = [Query.Definition: Query]()

        for definition: Query.Definition in [.highlights, .locals, .injections] {
            var combined = Data()

            // Load base query file
            let baseFile = baseURL.appendingPathComponent(definition.filename)
            if let data = try? Data(contentsOf: baseFile) {
                combined.append(data)
                combined.append(Data("\n".utf8))
            }

            // Load extra base files (e.g., highlights-jsx.scm for TSX)
            if definition == .highlights {
                for extra in extraBaseHighlights {
                    let extraFile = baseURL.appendingPathComponent("\(extra).scm")
                    if let data = try? Data(contentsOf: extraFile) {
                        combined.append(data)
                        combined.append(Data("\n".utf8))
                    }
                }
            }

            // Load overlay query file
            let overlayFile = overlayURL.appendingPathComponent(definition.filename)
            if let data = try? Data(contentsOf: overlayFile) {
                combined.append(data)
            }

            if !combined.isEmpty {
                queries[definition] = try Query(language: language, data: combined)
            }
        }

        return queries
    }

    private static func queriesDirectoryURL(for bundleName: String) -> URL? {
        guard let bundlePath = Bundle.main.url(forResource: bundleName, withExtension: "bundle") else { return nil }
        let short = bundlePath.appendingPathComponent("queries", isDirectory: true)
        if FileManager.default.fileExists(atPath: short.path) { return short }
        return bundlePath.appendingPathComponent("Contents/Resources/queries", isDirectory: true)
    }

    private enum MergedQueryError: Error {
        case bundleNotFound
    }

    // MARK: - Token Attribute Provider

    private static let attributeProvider: TokenAttributeProvider = { token in
        let name = token.name
        let color: NSColor
        let font: NSFont

        if name.hasPrefix("keyword") {
            color = EditorTheme.keyword
            font = boldFont
        } else if name.hasPrefix("string") {
            color = EditorTheme.string
            font = regularFont
        } else if name.hasPrefix("comment") {
            color = EditorTheme.comment
            font = regularFont
        } else if name.hasPrefix("number") || name.hasPrefix("float") || name.hasPrefix("integer") {
            color = EditorTheme.number
            font = regularFont
        } else if name.hasPrefix("type") || name.hasPrefix("constructor") {
            color = EditorTheme.type
            font = regularFont
        } else if name.hasPrefix("function") || name.hasPrefix("method") {
            color = EditorTheme.function
            font = regularFont
        } else if name.hasPrefix("property") || name.hasPrefix("field") {
            color = EditorTheme.property
            font = regularFont
        } else if name.hasPrefix("tag") {
            color = EditorTheme.keyword
            font = boldFont
        } else if name.hasPrefix("attribute") || name.hasPrefix("include") || name.hasPrefix("preproc") {
            color = EditorTheme.preprocessor
            font = regularFont
        } else if name.hasPrefix("variable") || name.hasPrefix("constant") {
            color = EditorTheme.type
            font = regularFont
        } else if name.hasPrefix("label") {
            color = EditorTheme.property
            font = regularFont
        } else if name.hasPrefix("operator") || name.hasPrefix("punctuation") {
            color = .secondaryLabelColor
            font = regularFont
        } else if name.hasPrefix("text.title") {
            color = EditorTheme.keyword
            font = boldFont
        } else if name.hasPrefix("text.literal") {
            color = EditorTheme.string
            font = regularFont
        } else if name.hasPrefix("text.uri") {
            color = EditorTheme.function
            font = regularFont
        } else if name.hasPrefix("text.reference") {
            color = EditorTheme.type
            font = regularFont
        } else if name.hasPrefix("text.emphasis") {
            color = .labelColor
            font = italicFont
        } else if name.hasPrefix("text.strong") {
            color = .labelColor
            font = boldFont
        } else if name.hasPrefix("text") {
            color = .labelColor
            font = regularFont
        } else {
            color = .labelColor
            font = regularFont
        }

        return [.font: font, .foregroundColor: color]
    }
}
