import SwiftUI

struct ChatAreaView: View {
    @Bindable var chatState: ChatState

    var body: some View {
        VStack(spacing: 0) {
            if chatState.messages.isEmpty {
                emptyState
            } else {
                messageList
            }

            if let error = chatState.streamError {
                errorBanner(error)
            }

            ChatInputView(chatState: chatState)
        }
    }

    private var emptyState: some View {
        VStack(spacing: 16) {
            Spacer()
            Image(systemName: "bubble.left.and.bubble.right.fill")
                .font(.system(size: 48))
                .foregroundStyle(.quaternary)
            Text("Point Guard")
                .font(.system(size: 28, weight: .bold))
                .foregroundStyle(.secondary)
            Text("Direct your work from here.")
                .font(.system(size: 15))
                .foregroundStyle(.tertiary)
            Spacer()
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }

    private var messageList: some View {
        ScrollViewReader { proxy in
            ScrollView {
                LazyVStack(spacing: 0) {
                    ForEach(chatState.messages.indices, id: \.self) { index in
                        MessageStreamView(message: chatState.messages[index])
                            .padding(.top, topPadding(for: index))
                            .id(chatState.messages[index].id)
                    }
                }
                .padding(20)
            }
            .onChange(of: chatState.messages.count) { _, _ in
                if let last = chatState.messages.last {
                    withAnimation(.easeOut(duration: 0.2)) {
                        proxy.scrollTo(last.id, anchor: .bottom)
                    }
                }
            }
            .onChange(of: chatState.messages.last?.contentBlocks.count) { _, _ in
                if let last = chatState.messages.last {
                    proxy.scrollTo(last.id, anchor: .bottom)
                }
            }
        }
    }

    private func topPadding(for index: Int) -> CGFloat {
        guard index > 0 else { return 0 }
        let current = chatState.messages[index].role
        let previous = chatState.messages[index - 1].role
        switch (previous, current) {
        case (.user, .user), (.assistant, .assistant):
            return PurePointTheme.inlineSpacing
        default:
            return PurePointTheme.turnSpacing
        }
    }

    private func errorBanner(_ message: String) -> some View {
        InlineErrorBanner(message: message) {
            chatState.streamError = nil
        }
    }
}
