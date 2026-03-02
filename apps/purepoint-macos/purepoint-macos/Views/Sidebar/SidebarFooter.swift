import SwiftUI

struct SidebarFooter: View {
    @Environment(AppState.self) private var appState
    let selection: SidebarSelection?

    var body: some View {
        VStack(spacing: 0) {
            Divider()

            HStack {
                Button {
                    // No-op
                } label: {
                    Image(systemName: "gear")
                        .foregroundStyle(.secondary)
                }
                .buttonStyle(.plain)

                Text("Settings")
                    .font(PurePointTheme.smallFont)
                    .foregroundStyle(.secondary)

                Spacer()

                Button {
                    showCommandPalette()
                } label: {
                    HStack(spacing: 3) {
                        Image(systemName: "plus")
                        Text("Add")
                            .font(PurePointTheme.smallFont)
                    }
                    .foregroundStyle(.secondary)
                }
                .buttonStyle(.plain)
            }
            .padding(.horizontal, PurePointTheme.padding + 4)
            .frame(height: PurePointTheme.footerHeight)
        }
    }

    private func showCommandPalette() {
        let state = appState
        let sel = selection
        CommandPalettePanel.show(relativeTo: NSApp.keyWindow) { variant, prompt in
            state.createAgent(variant: variant, prompt: prompt, selection: sel)
        }
    }
}

#Preview {
    SidebarFooter(selection: nil)
        .frame(width: 240)
        .environment(AppState())
}
