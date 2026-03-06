import SwiftUI

struct PointGuardView: View {
    @Binding var selection: SidebarSelection?
    @Environment(AppState.self) private var appState
    @State private var chatState = ChatState(processProvider: ClaudeProcess())
    @State private var showSidebar = true

    var body: some View {
        HSplitView {
            if showSidebar {
                ConversationSidebarView(chatState: chatState)
                    .frame(minWidth: 240, idealWidth: 280, maxWidth: 360)
            }

            ChatAreaView(chatState: chatState)
                .frame(maxWidth: .infinity, maxHeight: .infinity)
        }
        .task {
            await chatState.refreshSessions()
        }
        .onKeyPress(characters: .init(charactersIn: "s"), phases: .down) { press in
            if press.modifiers.contains([.command, .shift]) {
                showSidebar.toggle()
                return .handled
            }
            return .ignored
        }
    }
}
