#if os(macOS)
import SwiftUI

struct CodeBlockView: View {
    let language: String?
    let code: String
    var body: some View { ChatMarkdownCodeView(language: language ?? "", code: code) }
}
#endif
