import SwiftUI

struct SettingsGeneralView: View {
    @Environment(SettingsState.self) private var settingsState

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
        }
    }
}
