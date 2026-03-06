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
        .toolbar {
            ToolbarItem(placement: .navigation) {
                Button {
                    withAnimation(.easeInOut(duration: 0.2)) {
                        showSidebar.toggle()
                    }
                } label: {
                    Image(systemName: "sidebar.left")
                }
                .help(showSidebar ? "Hide conversations (⌘⇧S)" : "Show conversations (⌘⇧S)")
            }
        }
        .task {
            await chatState.refreshSessions()
        }
        .onReceive(NotificationCenter.default.publisher(for: .toggleChatSidebar)) { _ in
            showSidebar.toggle()
        }
    }
}
