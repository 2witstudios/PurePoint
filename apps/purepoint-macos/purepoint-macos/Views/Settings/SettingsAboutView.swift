import SwiftUI

struct SettingsAboutView: View {
    @EnvironmentObject var updaterViewModel: CheckForUpdatesViewModel

    var body: some View {
        VStack(spacing: 24) {
            Spacer()

            VStack(spacing: 12) {
                Image("PurePointLogo")
                    .resizable()
                    .aspectRatio(contentMode: .fit)
                    .frame(width: 64, height: 64)
                    .clipShape(RoundedRectangle(cornerRadius: 14))

                Text("PurePoint")
                    .font(.system(size: 22, weight: .bold))

                Text("Agent-first coding workspace")
                    .font(.system(size: 13))
                    .foregroundStyle(.secondary)
            }

            GroupBox {
                HStack {
                    Text("Version")
                        .font(.system(size: 13))
                    Spacer()
                    Text(versionString)
                        .font(.system(size: 12, design: .monospaced))
                        .foregroundStyle(.secondary)
                }
                .padding(.vertical, 4)
                Divider()
                HStack {
                    Text("Build")
                        .font(.system(size: 13))
                    Spacer()
                    Text(buildString)
                        .font(.system(size: 12, design: .monospaced))
                        .foregroundStyle(.secondary)
                }
                .padding(.vertical, 4)
            }
            .groupBoxStyle(SettingsGroupBoxStyle())
            .frame(maxWidth: 300)

            Button("Check for Updates\u{2026}") {
                updaterViewModel.checkForUpdates()
            }
            .disabled(!updaterViewModel.canCheckForUpdates)

            HStack(spacing: 16) {
                Link("Documentation", destination: URL(string: "https://purepoint.dev/docs")!)
                    .font(.system(size: 12))
                Link("GitHub", destination: URL(string: "https://github.com/purepoint-dev/purepoint")!)
                    .font(.system(size: 12))
            }

            Text("\u{00A9} 2026 PurePoint")
                .font(.system(size: 11))
                .foregroundStyle(.tertiary)

            Spacer()
        }
        .frame(maxWidth: .infinity)
    }

    private var versionString: String { Bundle.main.appVersion }
    private var buildString: String { Bundle.main.appBuild }
}
