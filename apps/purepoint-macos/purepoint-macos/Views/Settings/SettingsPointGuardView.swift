import SwiftUI

struct SettingsPointGuardView: View {
    @Environment(AppState.self) private var appState
    @Environment(SettingsState.self) private var settingsState

    var body: some View {
        @Bindable var settings = settingsState

        VStack(alignment: .leading, spacing: 24) {
            Text("Point Guard")
                .font(.system(size: 18, weight: .semibold))

            PiChatConnectionSettingsView(model: appState.pointGuardChat)

            GroupBox("Shell mode") {
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

private struct PiChatConnectionSettingsView: View {
    @ObservedObject var model: PiChatModel
    @State private var endpoint = ""
    @AppStorage("PP_pointGuardSecretPath") private var secretPath = "~/.config/pi-mobile/pairing-secret"

    var body: some View {
        GroupBox("Pi chat connection") {
            VStack(alignment: .leading, spacing: 10) {
                TextField("Bridge address", text: $endpoint)
                    .textFieldStyle(.roundedBorder)
                TextField("Pairing secret file", text: $secretPath)
                    .textFieldStyle(.roundedBorder)
                Text(
                    "Use the address of your running Pi bridge, ending in /v1. Desktop and phone share the same live Pi conversation."
                )
                .font(.system(size: 11)).foregroundStyle(.secondary)
                HStack {
                    Button("Connect") {
                        let path = NSString(string: secretPath).expandingTildeInPath
                        let address = endpoint
                        Task {
                            let secret = await Task.detached(priority: .userInitiated) {
                                (try? String(contentsOfFile: path, encoding: .utf8))?.trimmingCharacters(
                                    in: .whitespacesAndNewlines) ?? ""
                            }.value
                            guard !secret.isEmpty else {
                                model.error =
                                    "Could not read the pairing secret file. Check its path and start the Pi bridge first."
                                return
                            }
                            model.pair(endpoint: address, secret: secret)
                        }
                    }
                    Button("Disconnect") { model.disconnect() }
                    Text(model.connectionStatus).font(.system(size: 11)).foregroundStyle(.secondary)
                }
                if let error = model.error {
                    Text(error).font(.system(size: 11)).foregroundStyle(.red)
                }
            }.padding(8)
        }
        .onAppear { endpoint = model.endpoint.isEmpty ? "ws://127.0.0.1:8787/v1" : model.endpoint }
        .groupBoxStyle(SettingsGroupBoxStyle())
    }
}
