import SwiftUI

struct PointGuardView: View {
    @Binding var selection: SidebarSelection?
    @Environment(AppState.self) private var appState
    @State private var chatState = ChatState(processProvider: ClaudeProcess())
    @State private var showSidebar = true

    var body: some View {
        HSplitView {
            if showSidebar {
                ConversationSidebarView(chatState: chatState, showSidebar: $showSidebar)
                    .frame(minWidth: 180, idealWidth: 200, maxWidth: 280)
            }

            ChatAreaView(chatState: chatState)
                .frame(maxWidth: .infinity, maxHeight: .infinity)
        }
        .toolbar {
            if !showSidebar {
                ToolbarItem(placement: .navigation) {
                    Button {
                        withAnimation(.easeInOut(duration: 0.2)) {
                            showSidebar = true
                        }
                    } label: {
                        Image(systemName: "sidebar.left")
                    }
                    .help("Show conversations (⌘⇧S)")
                }
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
