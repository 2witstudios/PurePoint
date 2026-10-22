import SwiftUI

struct SettingsGeneralView: View {
    @Environment(SettingsState.self) private var settingsState
    @Environment(AppState.self) private var appState

    @AppStorage("PP_signal_permissionNotifications") private var permissionNotifications = false
    @AppStorage("PP_signal_permissionSound") private var permissionSound = false
    @AppStorage("PP_signal_questionNotifications") private var questionNotifications = false
    @AppStorage("PP_signal_questionSound") private var questionSound = false
    @AppStorage("PP_signal_errorNotifications") private var errorNotifications = false
    @AppStorage("PP_signal_errorSound") private var errorSound = false
    @AppStorage("PP_signal_completionNotifications") private var completionNotifications = false
    @AppStorage("PP_signal_completionSound") private var completionSound = false

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

                Toggle("Use point guard names", isOn: $settings.usePointGuardNames)
                    .padding(.vertical, 8)

                Text("Turn off to name each agent or terminal before launching it.")
                    .font(.caption)
                    .foregroundStyle(.secondary)

                // A login item is a Mac concept; the iPad hides the row (REQ-IPAD-040).
                #if os(macOS)
                Divider()

                Toggle("Launch at login", isOn: $settings.launchAtLogin)
                    .padding(.vertical, 8)
                #endif
            }
            .groupBoxStyle(SettingsGroupBoxStyle())

            GroupBox("Agent notifications") {
                VStack(alignment: .leading, spacing: 8) {
                    Toggle("Permission notifications", isOn: $permissionNotifications)
                    Toggle("Permission sound", isOn: $permissionSound)
                    Toggle("Question notifications", isOn: $questionNotifications)
                    Toggle("Question sound", isOn: $questionSound)
                    Toggle("Error notifications", isOn: $errorNotifications)
                    Toggle("Error sound", isOn: $errorSound)
                    Toggle("Completion notifications", isOn: $completionNotifications)
                    Toggle("Completion sound", isOn: $completionSound)
                    Text("Alerts become available for qualified agent signals. Permission and question alerts require a verified route to the exact request. iOS background push is not available.")
                        .font(.caption).foregroundStyle(.secondary)
                }
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
            await appState.agentsHubState.loadAll(projectRoots: appState.libraryProjectRoots)
        }
    }
}
