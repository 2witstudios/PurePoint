import SwiftUI

struct MessageBubbleView: View {
    let message: ChatMessage

    var body: some View {
        switch message.role {
        case .user:
            userBubble
        case .assistant:
            assistantBubble
        }
    }

    private var userBubble: some View {
        HStack {
            Spacer(minLength: 80)
            VStack(alignment: .trailing, spacing: 4) {
                ForEach(message.contentBlocks) { block in
                    if case .text(_, let text) = block {
                        Text(text)
                            .font(.system(size: 14))
                            .textSelection(.enabled)
                    }
                }
            }
            .padding(.horizontal, 14)
            .padding(.vertical, 10)
            .background(Color.accentColor.opacity(0.15), in: RoundedRectangle(cornerRadius: 16))
        }
    }

    private var assistantBubble: some View {
        VStack(alignment: .leading, spacing: 8) {
            ForEach(message.contentBlocks) { block in
                ContentBlockView(block: block)
            }

            if message.isStreaming {
                streamingCursor
            }
        }
        .padding(.horizontal, 14)
        .padding(.vertical, 12)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(Color(NSColor.controlBackgroundColor), in: RoundedRectangle(cornerRadius: 12))
    }

    private var streamingCursor: some View {
        HStack(spacing: 4) {
            Circle()
                .fill(Color.accentColor)
                .frame(width: 6, height: 6)
                .opacity(0.6)
            Circle()
                .fill(Color.accentColor)
                .frame(width: 6, height: 6)
                .opacity(0.4)
            Circle()
                .fill(Color.accentColor)
                .frame(width: 6, height: 6)
                .opacity(0.2)
        }
        .animation(.easeInOut(duration: 0.8).repeatForever(autoreverses: true), value: message.isStreaming)
    }
}
