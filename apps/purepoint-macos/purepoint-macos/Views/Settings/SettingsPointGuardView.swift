import SwiftUI

struct SettingsPointGuardView: View {
    @Environment(SettingsState.self) private var settingsState

    var body: some View {
        @Bindable var settings = settingsState

        VStack(alignment: .leading, spacing: 24) {
            Text("Point Guard")
                .font(.system(size: 18, weight: .semibold))

            GroupBox {
                VStack(alignment: .leading, spacing: 4) {
                    TextField("Launch command", text: $settings.pointGuardLaunchCommand)
                        .textFieldStyle(.roundedBorder)

                    Text("Command to run when the terminal starts. Leave empty for a plain shell.")
                        .font(.system(size: 11))
                        .foregroundStyle(.tertiary)
                }
                .padding(.vertical, 8)

                Divider()

                VStack(alignment: .leading, spacing: 2) {
                    Toggle("Auto-approve all actions", isOn: $settings.pointGuardSkipPermissions)
                    Text("Skip permission prompts so the agent runs without interruptions")
                        .font(.system(size: 11))
                        .foregroundStyle(.tertiary)
                }
                .padding(.vertical, 8)
            }
            .groupBoxStyle(SettingsGroupBoxStyle())
        }
    }
}
