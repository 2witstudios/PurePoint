import SwiftUI

struct SettingsView: View {
    @Environment(SettingsState.self) private var settingsState
    @State private var selectedSection: SettingsSection = .general

    var body: some View {
        HStack(spacing: 0) {
            sidebar
            Divider()
            content
        }
        .frame(
            width: PurePointTheme.settingsWidth,
            height: PurePointTheme.settingsHeight
        )
        .background(.regularMaterial)
    }

    // MARK: - Sidebar

    private var sidebar: some View {
        VStack(alignment: .leading, spacing: 0) {
            Text("Settings")
                .font(.system(size: 13, weight: .semibold))
                .foregroundStyle(.secondary)
                .padding(.horizontal, 16)
                .padding(.top, 20)
                .padding(.bottom, 12)

            ForEach(SettingsSection.allCases) { section in
                sidebarRow(section)
            }

            Spacer()

            Text(versionString)
                .font(.system(size: 10))
                .foregroundStyle(.tertiary)
                .padding(.horizontal, 16)
                .padding(.bottom, 12)
        }
        .frame(width: PurePointTheme.settingsSidebarWidth)
        .background(.ultraThinMaterial)
    }

    private func sidebarRow(_ section: SettingsSection) -> some View {
        Button {
            withAnimation(.easeInOut(duration: 0.15)) {
                selectedSection = section
            }
        } label: {
            HStack(spacing: 8) {
                Image(systemName: section.icon)
                    .font(.system(size: 12))
                    .frame(width: 16, height: 16)
                Text(section.title)
                    .font(.system(size: 13))
                Spacer()
            }
            .padding(.horizontal, 10)
            .padding(.vertical, 6)
            .background(
                selectedSection == section
                    ? Color.accentColor.opacity(0.15)
                    : Color.clear
            )
            .clipShape(RoundedRectangle(cornerRadius: 6))
        }
        .buttonStyle(.plain)
        .padding(.horizontal, 8)
    }

    // MARK: - Content

    private var content: some View {
        ScrollView {
            Group {
                switch selectedSection {
                case .general:
                    SettingsGeneralView()
                case .hotkeys:
                    SettingsHotkeysView()
                case .display:
                    SettingsDisplayView()
                case .about:
                    SettingsAboutView()
                }
            }
            .padding(24)
            .frame(maxWidth: .infinity, alignment: .leading)
        }
    }

    // MARK: - Helpers

    private var versionString: String {
        let version = Bundle.main.infoDictionary?["CFBundleShortVersionString"] as? String ?? "–"
        return "v\(version)"
    }
}

// MARK: - Grouped Card Style

struct SettingsGroupBoxStyle: GroupBoxStyle {
    func makeBody(configuration: Configuration) -> some View {
        VStack(alignment: .leading, spacing: 0) {
            configuration.label
                .font(.system(size: 12, weight: .medium))
                .foregroundStyle(.secondary)
                .padding(.bottom, 8)

            VStack(alignment: .leading, spacing: 0) {
                configuration.content
            }
            .padding(12)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(Color(nsColor: .controlBackgroundColor).opacity(0.5))
            .clipShape(RoundedRectangle(cornerRadius: 8))
        }
    }
}
