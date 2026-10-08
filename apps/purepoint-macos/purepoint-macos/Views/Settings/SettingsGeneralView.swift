import SwiftUI

struct SettingsGeneralView: View {
    @Environment(SettingsState.self) private var settingsState
    @Environment(AppState.self) private var appState

    private var paletteItems: [CommandPaletteItem] {
        CommandPaletteItem.buildItems(
            builtInVariants: AgentVariant.variantsWithWorktree,
            agents: appState.agentsHubState.agents,
            swarms: appState.agentsHubState.swarms,
            preferredOrder: settingsState.commandPaletteOrder
        )
    }

    var body: some View {
        @Bindable var settings = settingsState

        VStack(alignment: .leading, spacing: 24) {
            Text("General")
                .font(.system(size: 18, weight: .semibold))

            GroupBox {
                Toggle("Restore projects on launch", isOn: $settings.restoreProjectsOnLaunch)
                    .padding(.vertical, 8)

                Divider()

                Toggle("Launch at login", isOn: $settings.launchAtLogin)
                    .padding(.vertical, 8)
            }
            .groupBoxStyle(SettingsGroupBoxStyle())

            GroupBox {
                let items = paletteItems
                ForEach(Array(items.enumerated()), id: \.element.id) { index, item in
                    if index > 0 { Divider() }
                    HStack(spacing: 8) {
                        Image(systemName: item.icon)
                            .frame(width: 18)
                            .foregroundStyle(.secondary)
                        Text(item.displayName)
                        if let category = item.categoryLabel {
                            Text(category)
                                .font(.caption)
                                .foregroundStyle(.secondary)
                        }
                        Spacer()
                        Button {
                            settingsState.moveCommandPaletteItem(item.id, by: -1, items: items)
                        } label: {
                            Image(systemName: "chevron.up")
                        }
                        .disabled(index == 0)
                        .help("Move \(item.displayName) up")
                        .accessibilityLabel("Move \(item.displayName) up")

                        Button {
                            settingsState.moveCommandPaletteItem(item.id, by: 1, items: items)
                        } label: {
                            Image(systemName: "chevron.down")
                        }
                        .disabled(index == items.count - 1)
                        .help("Move \(item.displayName) down")
                        .accessibilityLabel("Move \(item.displayName) down")
                    }
                    .controlSize(.small)
                    .padding(.vertical, 6)
                }
            } label: {
                HStack {
                    Text("Command Palette Order")
                    Spacer()
                    Button("Reset") { settings.commandPaletteOrder = [] }
                        .disabled(settings.commandPaletteOrder.isEmpty)
                }
            }
            .groupBoxStyle(SettingsGroupBoxStyle())

            Text(
                "Use the arrows to change the order in ⌘N. The first entry is selected by default. Custom commands enabled in the Agents Hub appear here too."
            )
            .font(.caption)
            .foregroundStyle(.secondary)
        }
        .task {
            await appState.agentsHubState.loadAll(projectRoots: appState.projects.map(\.projectRoot))
        }
    }
}
