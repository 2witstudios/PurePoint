import SwiftUI

struct SettingsGeneralView: View {
    @Environment(SettingsState.self) private var settingsState

    var body: some View {
        @Bindable var settings = settingsState

        VStack(alignment: .leading, spacing: 24) {
            Text("General")
                .font(.system(size: 18, weight: .semibold))

            GroupBox {
                LabeledContent("Default Agent") {
                    Picker("", selection: $settings.defaultAgentVariant) {
                        ForEach(AgentVariant.allVariants) { variant in
                            Text(variant.displayName).tag(variant.id)
                        }
                    }
                    .labelsHidden()
                    .frame(width: 160)
                }
                .padding(.vertical, 8)

                Divider()

                LabeledContent("Project Directory") {
                    HStack(spacing: 8) {
                        Text(settings.defaultProjectDirectory)
                            .font(.system(size: 12, design: .monospaced))
                            .foregroundStyle(.secondary)
                            .lineLimit(1)
                            .truncationMode(.middle)

                        Button("Choose\u{2026}") {
                            chooseDirectory()
                        }
                    }
                }
                .padding(.vertical, 8)

                Divider()

                Toggle("Restore projects on launch", isOn: $settings.restoreProjectsOnLaunch)
                    .padding(.vertical, 8)

                Divider()

                Toggle("Launch at login", isOn: $settings.launchAtLogin)
                    .padding(.vertical, 8)
            }
            .groupBoxStyle(SettingsGroupBoxStyle())
        }
    }

    private func chooseDirectory() {
        let panel = NSOpenPanel()
        panel.canChooseDirectories = true
        panel.canChooseFiles = false
        panel.allowsMultipleSelection = false
        panel.message = "Choose default project directory"

        if panel.runModal() == .OK, let url = panel.url {
            settingsState.defaultProjectDirectory = url.path
        }
    }
}
