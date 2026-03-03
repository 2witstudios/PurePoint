import SwiftUI

struct SettingsAboutView: View {
    var body: some View {
        VStack(spacing: 24) {
            Spacer()

            VStack(spacing: 12) {
                Image(systemName: "point.3.connected.trianglepath.dotted")
                    .font(.system(size: 48))
                    .foregroundStyle(.secondary)

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
                Divider()
                HStack {
                    Text("Build")
                        .font(.system(size: 13))
                    Spacer()
                    Text(buildString)
                        .font(.system(size: 12, design: .monospaced))
                        .foregroundStyle(.secondary)
                }
            }
            .groupBoxStyle(SettingsGroupBoxStyle())
            .frame(maxWidth: 300)

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

    private var versionString: String {
        Bundle.main.infoDictionary?["CFBundleShortVersionString"] as? String ?? "–"
    }

    private var buildString: String {
        Bundle.main.infoDictionary?["CFBundleVersion"] as? String ?? "–"
    }
}
