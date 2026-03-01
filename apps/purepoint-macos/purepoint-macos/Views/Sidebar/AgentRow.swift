import SwiftUI

struct AgentRow: View {
    let agent: MockAgent

    var body: some View {
        Label {
            Text(agent.name)
                .font(PurePointTheme.smallFont)
        } icon: {
            Circle()
                .fill(agent.status.color)
                .frame(width: PurePointTheme.statusDotSize, height: PurePointTheme.statusDotSize)
        }
    }
}

#Preview {
    VStack(alignment: .leading) {
        AgentRow(agent: MockAgent(id: "1", name: "Agent 1", status: .running))
        AgentRow(agent: MockAgent(id: "2", name: "Agent 2", status: .completed))
        AgentRow(agent: MockAgent(id: "3", name: "Agent 3", status: .failed))
    }
    .padding()
    .preferredColorScheme(.dark)
}
