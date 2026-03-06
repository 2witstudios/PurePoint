import SwiftUI

struct MarkdownTextView: View {
    let text: String
    private let attributed: AttributedString?

    init(text: String) {
        self.text = text
        self.attributed = try? AttributedString(markdown: text, options: .init(interpretedSyntax: .full))
    }

    var body: some View {
        if let attributed {
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
