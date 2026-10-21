#if os(iOS)
import SwiftUI
import UIKit
import UniformTypeIdentifiers

/// iOS twin of the Mac's Add Machine sheet, with its layout and styles. The iPad has no ssh
/// config, agent or keys of its own, so it collects host, port and user instead of an ssh
/// host, shows the device public key to authorize on the machine, and asks the user to confirm
/// the machine's host key on first connect (REQ-IPAD-029, REQ-IPAD-030). The machine is saved
/// only after a health probe through the relay succeeds; the sheet then shows its row.
struct AddMachineSheet: View {
    @Environment(AppState.self) private var appState
    @Environment(\.dismiss) private var dismiss
    @State private var flow: AddMachineFlow
    @State private var showKeyImporter = false
    @State private var showPasteKey = false
    @State private var pastedKey = ""
    @State private var passphrase = ""
    @State private var copied = false

    /// `flow` is given when something else already drives it (the DEBUG launch argument).
    init(flow: AddMachineFlow? = nil) {
        _flow = State(initialValue: flow ?? AddMachineFlow())
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            Text("Add Machine").font(.headline)
            Text("Connect to PurePoint on another computer over SSH. Add this iPad's public key to ~/.ssh/authorized_keys on that computer first.")
                .font(.system(size: 11)).foregroundStyle(.secondary)
                .fixedSize(horizontal: false, vertical: true)

            if flow.step == .saved, let machine = savedMachine {
                MachineRowSummary(machine: machine)
            } else {
                fields
                deviceKey
            }

            if case .confirmHostKey(let key) = flow.step {
                hostKeyConfirmation(key)
            }
            if let notice = flow.notice {
                Label(notice, systemImage: flow.step == .tested || flow.step == .saved ? "checkmark.circle" : "info.circle")
                    .font(.system(size: 11))
                    .foregroundStyle(Color.green)
                    .fixedSize(horizontal: false, vertical: true)
            }
            if let error = flow.error {
                Text(error).font(.system(size: 11)).foregroundStyle(.red)
                    .fixedSize(horizontal: false, vertical: true)
            }

            buttons
        }
        .padding(20)
        .frame(width: 420)
        // Presented as a sheet, which does not inherit the root's Mac control defaults.
        .presentedControlDefaults()
        .onChange(of: flow.error) { _, error in if let error { Self.announce(error) } }
        .onChange(of: flow.step) { _, step in
            switch step {
            case .saved: Self.announce("Added \(flow.name)")
            case .tested: Self.announce(flow.notice ?? "Connected")
            case .confirmHostKey: Self.announce("Confirm the host key")
            default: break
            }
        }
        .fileImporter(isPresented: $showKeyImporter, allowedContentTypes: [.data, .text, .plainText]) { result in
            importKeyFile(result)
        }
        .sheet(isPresented: $showPasteKey) { pasteKeySheet.presentedControlDefaults() }
        .sheet(
            isPresented: Binding(
                // The paste sheet asks in place; a key from a file asks here.
                get: { flow.keyAwaitingPassphrase != nil && !showPasteKey },
                set: { shown in
                    if !shown { flow.cancelPassphrase() }
                })
        ) { passphraseSheet.presentedControlDefaults() }
    }

    // MARK: - Sections

    /// The Mac sheet's `Form` in the macOS columns style: right-aligned labels with a colon,
    /// rounded-border fields beside them. An iOS `Form` is an inset list that drops the labels.
    private var fields: some View {
        Grid(alignment: .leadingFirstTextBaseline, horizontalSpacing: 8, verticalSpacing: 8) {
            row("Name") { TextField("Name", text: $flow.name, prompt: Text("studio")) }
            row("Host") {
                TextField("Host", text: $flow.host, prompt: Text("studio.local or 192.168.1.20"))
                    .textInputAutocapitalization(.never)
                    .autocorrectionDisabled()
                    .keyboardType(.URL)
            }
            row("Port") {
                TextField("Port", text: $flow.port, prompt: Text("22"))
                    .keyboardType(.numberPad)
            }
            row("User") {
                TextField("User", text: $flow.user, prompt: Text("jono"))
                    .textInputAutocapitalization(.never)
                    .autocorrectionDisabled()
            }
        }
        .textFieldStyle(.roundedBorder)
        .disabled(flow.isBusy || isConfirming)
    }

    private func row<Field: View>(_ label: String, @ViewBuilder field: () -> Field) -> some View {
        GridRow {
            Text("\(label):")
                .gridColumnAlignment(.trailing)
                .accessibilityHidden(true)
            field()
        }
    }

    private var deviceKey: some View {
        VStack(alignment: .leading, spacing: 6) {
            Text("This iPad's public key").font(.system(size: 11, weight: .semibold))
            if let line = flow.publicKeyLine {
                Text(line)
                    .font(.system(size: 10, design: .monospaced))
                    .textSelection(.enabled)
                    .lineLimit(3)
                    .fixedSize(horizontal: false, vertical: true)
                    .accessibilityLabel("Public key")
            } else {
                Text("The device key could not be read from the Keychain.")
                    .font(.system(size: 11)).foregroundStyle(.red)
            }
            HStack {
                Button(copied ? "Copied" : "Copy Public Key") {
                    guard let line = flow.publicKeyLine else { return }
                    UIPasteboard.general.string = line
                    copied = true
                    Self.announce("Public key copied")
                }
                .disabled(flow.publicKeyLine == nil)
                Spacer()
                Menu("Import Key\u{2026}") {
                    Button("From File\u{2026}") { showKeyImporter = true }
                    Button("Paste\u{2026}") {
                        pastedKey = ""
                        showPasteKey = true
                    }
                }
                .fixedSize()
                .help("Use an OpenSSH Ed25519 private key instead of this iPad's own")
            }
            .font(.system(size: 11))
        }
    }

    private func hostKeyConfirmation(_ key: SSHPublicKey) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            Label("First connection to \(flow.host.trimmingCharacters(in: .whitespaces))", systemImage: "lock.shield")
                .font(.system(size: 11, weight: .semibold))
            Text("Check that this matches the machine's host key (run `ssh-keygen -lf /etc/ssh/ssh_host_ed25519_key.pub` there) before trusting it.")
                .font(.system(size: 11)).foregroundStyle(.secondary)
                .fixedSize(horizontal: false, vertical: true)
            Text("\(key.displayType)  \(key.fingerprint)")
                .font(.system(size: 11, design: .monospaced))
                .textSelection(.enabled)
                .accessibilityLabel("Host key \(key.displayType) \(key.fingerprint)")
            HStack {
                Spacer()
                Button("Don't Trust") { Task { await flow.cancel() } }
                Button("Trust and Connect") { Task { await flow.confirmHostKey(appState: appState) } }
                    .buttonStyle(.borderedProminent)
            }
        }
        .padding(10)
        .background(RoundedRectangle(cornerRadius: 6).fill(Color.orange.opacity(0.12)))
    }

    private var buttons: some View {
        HStack {
            if flow.step != .saved {
                Button(flow.isBusy && !savingAttempt ? "Testing\u{2026}" : "Test Connection") {
                    savingAttempt = false
                    Task { await flow.submit(appState: appState, save: false) }
                }
                .disabled(flow.isBusy || isConfirming || !canSubmit)
            }
            Spacer()
            if flow.step == .saved {
                Button("Done") { dismiss() }
                    .keyboardShortcut(.defaultAction)
                    .buttonStyle(.borderedProminent)
            } else {
                Button("Cancel") {
                    Task { await flow.cancel() }
                    dismiss()
                }
                .keyboardShortcut(.cancelAction)
                Button(flow.isBusy && savingAttempt ? "Adding\u{2026}" : "Add") {
                    savingAttempt = true
                    Task { await flow.submit(appState: appState) }
                }
                .keyboardShortcut(.defaultAction)
                .buttonStyle(.borderedProminent)
                .disabled(flow.isBusy || isConfirming || !canSubmit)
            }
        }
    }

    @State private var savingAttempt = true

    private var pasteKeySheet: some View {
        VStack(alignment: .leading, spacing: 14) {
            Text("Import Private Key").font(.headline)
            Text("Paste an OpenSSH Ed25519 private key. A protected key asks for its passphrase next. It replaces this iPad's key for every machine and stays in this device's Keychain.")
                .font(.system(size: 11)).foregroundStyle(.secondary)
                .fixedSize(horizontal: false, vertical: true)
            TextEditor(text: $pastedKey)
                .font(.system(size: 10, design: .monospaced))
                .textInputAutocapitalization(.never)
                .autocorrectionDisabled()
                .frame(minHeight: 160)
                .overlay(RoundedRectangle(cornerRadius: 4).stroke(Color.secondary.opacity(0.3)))
                .disabled(flow.keyAwaitingPassphrase != nil)
            if flow.keyAwaitingPassphrase != nil {
                SecureField("Passphrase", text: $passphrase)
                    .textContentType(.password)
                    .onSubmit(importPasted)
                if let error = flow.error {
                    Text(error).font(.system(size: 11)).foregroundStyle(.red)
                }
            }
            HStack {
                Spacer()
                Button("Cancel") {
                    pastedKey = ""
                    passphrase = ""
                    flow.cancelPassphrase()
                    showPasteKey = false
                }
                Button("Import", action: importPasted)
                    .disabled(pastedKey.isEmpty || (flow.keyAwaitingPassphrase != nil && passphrase.isEmpty))
            }
        }
        .padding(20)
        .frame(width: 420)
    }

    private var passphraseSheet: some View {
        VStack(alignment: .leading, spacing: 14) {
            Text("Key Passphrase").font(.headline)
            Text("This key is protected by a passphrase. PurePoint uses it once to decrypt the key and does not keep it.")
                .font(.system(size: 11)).foregroundStyle(.secondary)
                .fixedSize(horizontal: false, vertical: true)
            SecureField("Passphrase", text: $passphrase)
                .textContentType(.password)
                .onSubmit(importWithPassphrase)
            if let error = flow.error {
                Text(error).font(.system(size: 11)).foregroundStyle(.red)
            }
            HStack {
                Spacer()
                Button("Cancel") {
                    passphrase = ""
                    flow.cancelPassphrase()
                }
                Button("Import", action: importWithPassphrase)
                    .buttonStyle(.borderedProminent)
                    .disabled(passphrase.isEmpty)
            }
        }
        .padding(20)
        .frame(width: 380)
    }

    /// Import the pasted key; a protected one keeps the sheet open to ask for its passphrase.
    private func importPasted() {
        if flow.keyAwaitingPassphrase != nil {
            guard !passphrase.isEmpty else { return }
            flow.importAwaitingKey(passphrase: passphrase, appState: appState)
        } else {
            flow.importPrivateKey(pastedKey, appState: appState)
        }
        passphrase = ""
        copied = false
        guard flow.keyAwaitingPassphrase == nil else { return }
        pastedKey = ""
        showPasteKey = false
    }

    private func importWithPassphrase() {
        guard !passphrase.isEmpty else { return }
        flow.importAwaitingKey(passphrase: passphrase, appState: appState)
        passphrase = ""
        copied = false
    }

    // MARK: - Helpers

    private var isConfirming: Bool {
        if case .confirmHostKey = flow.step { return true }
        return false
    }

    private var canSubmit: Bool {
        !flow.name.trimmingCharacters(in: .whitespaces).isEmpty
            && !flow.host.trimmingCharacters(in: .whitespaces).isEmpty
            && !flow.user.trimmingCharacters(in: .whitespaces).isEmpty
    }

    private var savedMachine: MachineState? {
        appState.machines.first { $0.machine.id == flow.machineId }
    }

    private func importKeyFile(_ result: Result<URL, Error>) {
        guard case .success(let url) = result else { return }
        let scoped = url.startAccessingSecurityScopedResource()
        defer { if scoped { url.stopAccessingSecurityScopedResource() } }
        // A private key file is a few hundred bytes; anything large is not one.
        guard let data = try? Data(contentsOf: url), data.count < 64 * 1024 else {
            flow.importPrivateKey("", appState: appState)
            return
        }
        flow.importPrivateKey(String(decoding: data, as: UTF8.self), appState: appState)
        copied = false
    }

    /// Results appear in the sheet; VoiceOver users hear them too.
    private static func announce(_ message: String) {
        UIAccessibility.post(notification: .announcement, argument: message)
    }
}

/// A machine's row as the sidebar shows it: status dot, name, status, and its projects once
/// its daemon has listed them.
struct MachineRowSummary: View {
    let machine: MachineState

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            HStack(spacing: 6) {
                Circle().fill(Self.statusColor(machine.state)).frame(width: 7, height: 7)
                Image(systemName: "desktopcomputer").font(.system(size: 11)).foregroundStyle(.secondary)
                Text(machine.machine.name).font(.system(size: 12, weight: .semibold)).lineLimit(1)
                Spacer()
                Text(machine.statusText).font(.system(size: 10)).foregroundStyle(.secondary).lineLimit(1)
                if machine.actionError != nil {
                    Image(systemName: "exclamationmark.triangle.fill").font(.system(size: 10)).foregroundStyle(.orange)
                }
            }
            ForEach(machine.projects, id: \.projectId) { project in
                HStack(spacing: 6) {
                    Image(systemName: "folder").font(.system(size: 10)).foregroundStyle(.secondary)
                    Text(project.key.projectName).font(.system(size: 12)).lineLimit(1)
                    Spacer()
                    Text(project.projectRoot).font(.system(size: 10)).foregroundStyle(.tertiary).lineLimit(1)
                        .truncationMode(.head)
                }
                .padding(.leading, 20)
            }
            if case .offline(let reason) = machine.state {
                Text(reason).font(.system(size: 10)).foregroundStyle(.secondary)
                    .fixedSize(horizontal: false, vertical: true)
            }
            if let error = machine.actionError {
                Text(error).font(.system(size: 10)).foregroundStyle(.orange)
                    .fixedSize(horizontal: false, vertical: true)
            }
            if machine.state == .connected(.stopped) {
                Button(machine.isStartingDaemon ? "Starting Daemon\u{2026}" : "Start Daemon") { machine.startDaemon() }
                    .font(.system(size: 11))
                    .disabled(machine.isStartingDaemon)
            }
        }
        .padding(10)
        .background(RoundedRectangle(cornerRadius: 6).fill(Color.secondary.opacity(0.08)))
        .accessibilityElement(children: .combine)
    }

    static func statusColor(_ state: MachineConnectionState) -> Color {
        switch state {
        case .connected(.running): .green
        case .connected(.stopped): .orange
        case .connected(.incompatible): .red
        case .connecting: .yellow
        case .offline: .red
        case .disconnected: Color(uiColor: .tertiaryLabel)
        }
    }
}
#endif
