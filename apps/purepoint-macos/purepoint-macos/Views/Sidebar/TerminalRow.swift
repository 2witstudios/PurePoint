import SwiftUI

struct TerminalRow: View {
    let terminal: MockTerminal

    var body: some View {
        Label {
            Text(terminal.name)
                .font(PurePointTheme.smallFont)
        } icon: {
            Image(systemName: "terminal")
                .foregroundStyle(.secondary)
        }
    }
}

#Preview {
    TerminalRow(terminal: MockTerminal(id: "1", name: "Terminal 1"))
        .padding()
        .preferredColorScheme(.dark)
}
