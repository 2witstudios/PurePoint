import SwiftUI

struct AgentRow: View {
    let agent: AgentModel

    var body: some View {
        Label {
            Text(agent.displayName)
                .font(PurePointTheme.smallFont)
        } icon: {
            Circle()
                .fill(agent.status.color)
                .frame(width: PurePointTheme.statusDotSize, height: PurePointTheme.statusDotSize)
        }
    }
}
