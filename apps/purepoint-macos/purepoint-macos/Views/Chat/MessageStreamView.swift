import SwiftUI

struct MessageStreamView: View {
    let message: ChatMessage
    @State private var animating = false

    var body: some View {
        switch message.role {
        case .user:
            userMessage
        case .assistant:
            assistantMessage
        }
    }

    private var userMessage: some View {
        HStack {
            Spacer(minLength: 48)
            VStack(alignment: .leading, spacing: 4) {
                ForEach(message.contentBlocks) { block in
                    if case .text(_, let text) = block {
                        Text(text)
                            .font(PurePointTheme.chatFont).lineSpacing(3)
                            .textSelection(.enabled)
                    }
                }
            }
            .padding(.horizontal, 17).padding(.vertical, 13)
            .background(Color.primary.opacity(0.055), in: RoundedRectangle(cornerRadius: 22))
            .accessibilityLabel("Your message")
        }
        .frame(maxWidth: .infinity, alignment: .trailing)
    }

    private var assistantMessage: some View {
        VStack(alignment: .leading, spacing: 6) {
            ForEach(message.contentBlocks) { block in
                ContentBlockView(block: block, allBlocks: message.contentBlocks)
            }

            if message.isStreaming {
                streamingCursor
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    private var streamingCursor: some View {
        RoundedRectangle(cornerRadius: 1)
            .fill(Color.accentColor)
            .frame(width: 2, height: 16)
            .opacity(animating ? 1.0 : 0.2)
            .animation(
                .easeInOut(duration: 0.6).repeatForever(autoreverses: true),
                value: animating
            )
            .onAppear { animating = true }
    }
}
