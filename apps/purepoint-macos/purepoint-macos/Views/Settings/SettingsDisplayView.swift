import SwiftUI

struct SettingsDisplayView: View {
    @Environment(SettingsState.self) private var settingsState

    var body: some View {
        @Bindable var settings = settingsState

        VStack(alignment: .leading, spacing: 24) {
            Text("Display")
                .font(.system(size: 18, weight: .semibold))

            GroupBox("Appearance") {
                Picker("", selection: $settings.appearance) {
                    ForEach(AppAppearance.allCases, id: \.self) { option in
                        Text(option.label).tag(option)
                    }
                }
                .pickerStyle(.segmented)
                .labelsHidden()
            }
            .groupBoxStyle(SettingsGroupBoxStyle())

            GroupBox("Terminal") {
                HStack {
                    Text("Font Size")
                        .font(.system(size: 13))
                    Slider(value: $settings.terminalFontSize, in: 10...24, step: 1)
                    Text("\(Int(settings.terminalFontSize))pt")
                        .font(.system(size: 12, design: .monospaced))
                        .frame(width: 36, alignment: .trailing)
                }
            }
            .groupBoxStyle(SettingsGroupBoxStyle())

            GroupBox("Layout") {
                HStack {
                    Text("Grid Gap")
                        .font(.system(size: 13))
                    Slider(value: $settings.gridGap, in: 0...8, step: 1)
                    Text("\(Int(settings.gridGap))px")
                        .font(.system(size: 12, design: .monospaced))
                        .frame(width: 36, alignment: .trailing)
                }
            }
            .groupBoxStyle(SettingsGroupBoxStyle())
        }
    }
}
