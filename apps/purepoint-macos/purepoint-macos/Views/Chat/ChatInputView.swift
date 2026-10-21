#if os(macOS)
import SwiftUI

struct ChatInputView: View {
    @Bindable var chatState: ChatState
    @Environment(AppState.self) private var appState
    @FocusState private var isFocused: Bool

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            TextField("Message Point Guard…", text: $chatState.inputText, axis: .vertical)
                .textFieldStyle(.plain)
                .font(PurePointTheme.chatFont)
                .lineLimit(1...8)
                .fixedSize(horizontal: false, vertical: true)
                .focused($isFocused)
                .padding(.horizontal, 4).frame(minHeight: 34, alignment: .center)
                .accessibilityLabel("Message draft")
                .onKeyPress(keys: [.return], phases: .down) { press in
                    if press.modifiers.contains(.shift) { return .ignored }
                    if chatState.canSend { sendMessage() }
                    return .handled
                }
            HStack {
                Spacer(minLength: 0)
                actionButton
            }
        }
        .padding(16)
        .background(
            Color(NSColor.controlBackgroundColor),
            in: RoundedRectangle(cornerRadius: PurePointTheme.composerCornerRadius)
        )
        .overlay {
            RoundedRectangle(cornerRadius: PurePointTheme.composerCornerRadius)
                .strokeBorder(isFocused ? Color.accentColor.opacity(0.35) : Color.primary.opacity(0.08))
        }
        .frame(maxWidth: PurePointTheme.chatContentWidth)
        .frame(maxWidth: .infinity)
        .padding(.horizontal, PurePointTheme.chatHorizontalPadding)
        .padding(.top, 8).padding(.bottom, 16)
        .onAppear { isFocused = true }
    }

    @ViewBuilder
    private var actionButton: some View {
        if chatState.isStreaming {
            Button {
                Task { await chatState.stopStreaming() }
            } label: {
                actionIcon("stop.fill", enabled: true)
            }
            .buttonStyle(.plain)
            .accessibilityLabel("Stop streaming").help("Stop streaming")
        } else {
            Button {
                sendMessage()
            } label: {
                actionIcon("arrow.up", enabled: chatState.canSend)
            }
            .buttonStyle(.plain)
            .disabled(!chatState.canSend)
            .accessibilityLabel("Send message").help("Send message (Return)")
        }
    }

    private func actionIcon(_ name: String, enabled: Bool) -> some View {
        Image(systemName: name).font(.system(size: 14, weight: .semibold))
            .frame(width: 34, height: 34)
            .foregroundStyle(Color(NSColor.controlBackgroundColor))
            .background(enabled ? Color.accentColor : Color.secondary.opacity(0.35), in: Circle())
            .contentShape(Circle())
    }

    private func sendMessage() {
        let text = chatState.inputText
        chatState.inputText = ""

        let cwd =
            appState.activeLocalProjectRoot
            ?? FileManager.default.currentDirectoryPath
        Task {
            await chatState.send(text, cwd: cwd)
        }
    }
}
#endif
