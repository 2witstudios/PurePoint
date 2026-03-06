import SwiftUI

struct MarkdownTextView: View {
    let text: String

    var body: some View {
        if let attributed = try? AttributedString(markdown: text, options: .init(interpretedSyntax: .inlineOnlyPreservingWhitespace)) {
            Text(attributed)
                .font(.system(size: 14))
                .textSelection(.enabled)
        } else {
            Text(text)
                .font(.system(size: 14))
                .textSelection(.enabled)
        }
    }
}
