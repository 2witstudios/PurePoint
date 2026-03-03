import SwiftUI

struct SettingsHotkeysView: View {
    var body: some View {
        VStack(alignment: .leading, spacing: 24) {
            Text("Hotkeys")
                .font(.system(size: 18, weight: .semibold))

            GroupBox("Application") {
                hotkeyRow("New Agent", keys: ["⌘", "N"])
                Divider()
                hotkeyRow("Open Project", keys: ["⌘", "O"])
                Divider()
                hotkeyRow("Settings", keys: ["⌘", ","])
            }
            .groupBoxStyle(SettingsGroupBoxStyle())

            GroupBox("Panes") {
                hotkeyRow("Split Below", keys: ["⌘", "D"])
                Divider()
                hotkeyRow("Split Right", keys: ["⇧", "⌘", "D"])
                Divider()
                hotkeyRow("Close Pane", keys: ["⇧", "⌘", "W"])
                Divider()
                hotkeyRow("Focus Up", keys: ["⌥", "⌘", "↑"])
                Divider()
                hotkeyRow("Focus Down", keys: ["⌥", "⌘", "↓"])
                Divider()
                hotkeyRow("Focus Left", keys: ["⌥", "⌘", "←"])
                Divider()
                hotkeyRow("Focus Right", keys: ["⌥", "⌘", "→"])
            }
            .groupBoxStyle(SettingsGroupBoxStyle())

            Text("Custom keybindings coming in a future update.")
                .font(.system(size: 11))
                .foregroundStyle(.tertiary)
        }
    }

    private func hotkeyRow(_ action: String, keys: [String]) -> some View {
        HStack {
            Text(action)
                .font(.system(size: 13))
            Spacer()
            HStack(spacing: 4) {
                ForEach(keys, id: \.self) { key in
                    Text(key)
                        .font(.system(size: 12, design: .monospaced))
                        .padding(.horizontal, 6)
                        .padding(.vertical, 2)
                        .background(Color.secondary.opacity(0.1))
                        .clipShape(RoundedRectangle(cornerRadius: 4))
                }
            }
        }
        .padding(.vertical, 6)
    }
}
