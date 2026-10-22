#if os(macOS)
import SwiftUI

struct MarkdownTextView: View {
    let text: String
    var body: some View { ChatMarkdownView(text: text).equatable() }
}
#endif
