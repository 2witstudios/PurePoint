#if os(macOS)
import AppKit
import SwiftUI

/// Register a machine reachable over SSH. PurePoint stores only its name and SSH destination
/// (in `~/.pu/machines.json`, shared with `pu machine`); keys stay with ssh (MACH-001).
struct AddMachineSheet: View {
    @Environment(AppState.self) private var appState
    @Environment(\.dismiss) private var dismiss
    @State private var name = ""
    @State private var sshHost = ""
    @State private var aliases: [String] = []
    @State private var testResult: String?
    @State private var testSucceeded = false
    @State private var isTesting = false
    @State private var error: String?

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            Text("Add Machine").font(.headline)
            Text("Connect to PurePoint on another computer you can reach with ssh. Your ssh config, keys and agent are used as they are.")
                .font(.system(size: 11)).foregroundStyle(.secondary)
                .fixedSize(horizontal: false, vertical: true)

            Form {
                TextField("Name", text: $name, prompt: Text("studio"))
                HStack {
                    TextField("SSH host", text: $sshHost, prompt: Text("alias or user@host[:port]"))
                    if !aliases.isEmpty {
                        Menu("Hosts") {
                            ForEach(aliases, id: \.self) { alias in
                                Button(alias) {
                                    sshHost = alias
                                    if name.isEmpty { name = Self.suggestedName(alias) }
                                }
                            }
                        }
                        .fixedSize()
                        .help("Hosts from ~/.ssh/config")
                    }
                }
            }

            if let testResult {
                Label(testResult, systemImage: testSucceeded ? "checkmark.circle" : "exclamationmark.triangle")
                    .font(.system(size: 11))
                    .foregroundStyle(testSucceeded ? Color.green : Color.orange)
                    .fixedSize(horizontal: false, vertical: true)
            }
            if let error {
                Text(error).font(.system(size: 11)).foregroundStyle(.red)
                    .fixedSize(horizontal: false, vertical: true)
            }

            HStack {
                Button(isTesting ? "Testing\u{2026}" : "Test Connection", action: testConnection)
                    .disabled(isTesting || sshHost.isEmpty)
                Spacer()
                Button("Cancel") { dismiss() }
                    .keyboardShortcut(.cancelAction)
                Button("Add", action: add)
                    .keyboardShortcut(.defaultAction)
                    .disabled(name.isEmpty || sshHost.isEmpty)
            }
        }
        .padding(20)
        .frame(width: 420)
        .task { aliases = SSHConfig.hostAliases() }
    }

    static func suggestedName(_ alias: String) -> String {
        let candidate = alias.split(separator: "@").last.map(String.init) ?? alias
        let cleaned = candidate.split(separator: ":").first.map(String.init) ?? candidate
        return String(cleaned.unicodeScalars.filter { $0.isASCII && (CharacterSet.alphanumerics.contains($0) || "._-".unicodeScalars.contains($0)) })
    }

    private func testConnection() {
        let host = sshHost.trimmingCharacters(in: .whitespaces)
        do {
            try MachineRegistryStore.validate(sshHost: host)
        } catch {
            testSucceeded = false
            testResult = error.localizedDescription
            return
        }
        isTesting = true
        testResult = nil
        Task {
            do {
                let home = try await MachineSSH.resolveHome(sshHost: host)
                testSucceeded = true
                testResult = "Connected. Remote home: \(home)"
            } catch {
                testSucceeded = false
                testResult = error.localizedDescription
            }
            isTesting = false
            if let testResult { Self.announce(testResult) }
        }
    }

    private func add() {
        let trimmedName = name.trimmingCharacters(in: .whitespaces)
        let trimmedHost = sshHost.trimmingCharacters(in: .whitespaces)
        Task {
            do {
                try await appState.addMachine(name: trimmedName, sshHost: trimmedHost)
                Self.announce("Added \(trimmedName)")
                dismiss()
            } catch {
                self.error = error.localizedDescription
                Self.announce(error.localizedDescription)
            }
        }
    }

    /// Results appear in the sheet; VoiceOver users hear them too.
    private static func announce(_ message: String) {
        guard let window = NSApp.keyWindow ?? NSApp.mainWindow else { return }
        NSAccessibility.post(
            element: window, notification: .announcementRequested,
            userInfo: [.announcement: message, .priority: NSAccessibilityPriorityLevel.high.rawValue])
    }
}
#endif
