import AppKit
import Neon
import SwiftTreeSitter
import TreeSitterSwift
import TreeSitterRust
import TreeSitterJavaScript
import TreeSitterTypeScript
import TreeSitterPython
import TreeSitterJSON
import TreeSitterBash

@MainActor
final class SyntaxHighlightManager {
    private weak var textView: NSTextView?
    private var highlighter: TextViewHighlighter?
    private var currentLanguage: EditorLanguage = .plaintext

    private static let maxHighlightSize = 1_000_000
    private static var languageConfigs: [EditorLanguage: LanguageConfiguration] = [:]
    private static let regularFont = NSFont.monospacedSystemFont(ofSize: 13, weight: .regular)
    private static let boldFont = NSFont.monospacedSystemFont(ofSize: 13, weight: .bold)

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

        guard let textView else { return }

        if let storage = textView.textStorage, storage.length > Self.maxHighlightSize {
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
                attributeProvider: Self.attributeProvider
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
                config = try LanguageConfiguration(tree_sitter_typescript(), name: "TypeScript")
            case .python:
                config = try LanguageConfiguration(tree_sitter_python(), name: "Python")
            case .json:
                config = try LanguageConfiguration(tree_sitter_json(), name: "JSON")
            case .shell:
                config = try LanguageConfiguration(tree_sitter_bash(), name: "Bash")
            default:
                return nil
            }
        } catch {
            return nil
        }

        if let config { languageConfigs[language] = config }
        return config
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
        } else if name.hasPrefix("attribute") || name.hasPrefix("include") || name.hasPrefix("preproc") {
            color = EditorTheme.preprocessor
            font = regularFont
        } else if name.hasPrefix("operator") || name.hasPrefix("punctuation") {
            color = .secondaryLabelColor
            font = regularFont
        } else {
            color = .labelColor
            font = regularFont
        }

        return [.font: font, .foregroundColor: color]
    }
}
