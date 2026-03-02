import SwiftUI

struct AgentRow: View {
    let agent: AgentModel
    @Environment(AppState.self) private var appState
    @State private var showKillConfirmation = false

    var body: some View {
        Label {
            VStack(alignment: .leading, spacing: 1) {
                Text(agent.displayName)
                    .font(PurePointTheme.smallFont)
                Text(agent.agentType)
                    .font(.system(size: 9))
                    .foregroundStyle(.tertiary)
            }
        } icon: {
            Circle()
                .fill(agent.status.color)
                .frame(width: PurePointTheme.statusDotSize, height: PurePointTheme.statusDotSize)
        }
        .contextMenu {
            Button("Kill Agent", role: .destructive) {
                showKillConfirmation = true
            }
        }
        .confirmationDialog(
            "Kill \"\(agent.displayName)\"?",
            isPresented: $showKillConfirmation,
            titleVisibility: .visible
        ) {
            Button("Kill", role: .destructive) {
                appState.killAgent(agent.id)
            }
        }
    }
}
