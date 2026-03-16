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
                attributeProvider: Self.attributeProvider,
                locationTransformer: { _ in nil }
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
        let font: NSFont
        let color: NSColor

        if name.hasPrefix("keyword") {
            font = .monospacedSystemFont(ofSize: 13, weight: .bold)
            color = EditorTheme.keyword
        } else if name.hasPrefix("string") {
            font = .monospacedSystemFont(ofSize: 13, weight: .regular)
            color = EditorTheme.string
        } else if name.hasPrefix("comment") {
            font = .monospacedSystemFont(ofSize: 13, weight: .regular)
            color = EditorTheme.comment
        } else if name.hasPrefix("number") || name.hasPrefix("float") || name.hasPrefix("integer") {
            font = .monospacedSystemFont(ofSize: 13, weight: .regular)
            color = EditorTheme.number
        } else if name.hasPrefix("type") || name.hasPrefix("constructor") {
            font = .monospacedSystemFont(ofSize: 13, weight: .regular)
            color = EditorTheme.type
        } else if name.hasPrefix("function") || name.hasPrefix("method") {
            font = .monospacedSystemFont(ofSize: 13, weight: .regular)
            color = EditorTheme.function
        } else if name.hasPrefix("property") || name.hasPrefix("field") {
            font = .monospacedSystemFont(ofSize: 13, weight: .regular)
            color = EditorTheme.property
        } else if name.hasPrefix("attribute") || name.hasPrefix("include") || name.hasPrefix("preproc") {
            font = .monospacedSystemFont(ofSize: 13, weight: .regular)
            color = EditorTheme.preprocessor
        } else if name.hasPrefix("operator") || name.hasPrefix("punctuation") {
            font = .monospacedSystemFont(ofSize: 13, weight: .regular)
            color = .secondaryLabelColor
        } else {
            font = .monospacedSystemFont(ofSize: 13, weight: .regular)
            color = .labelColor
        }

        return [.font: font, .foregroundColor: color]
    }
}
