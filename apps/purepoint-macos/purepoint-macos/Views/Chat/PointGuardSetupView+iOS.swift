#if os(iOS)
import AVFoundation
import SwiftUI
import VisionKit

/// The iPad twin of the Mac Point Guard setup. The iPad never hosts Point Guard, so the Mac's
/// hosting, provider, working-folder and phone sections have no iOS form (REQ-IPAD-038). In their
/// place it pairs with a Mac by QR and lists the Macs it has paired with (REQ-IPAD-032, 035).
struct PointGuardSetupView: View {
    @ObservedObject var chat: PiChatModel
    @State private var showScanner = false
    @State private var requestingCamera = false
    @State private var renaming: PiPairedMac?
    @State private var machineName = ""
    #if DEBUG
    @State private var pastedCode = ""
    #endif

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            HStack {
                Label(chat.connectionStatus, systemImage: chat.connected ? "checkmark.circle" : "circle")
                Spacer()
                if chat.connected {
                    Button("Disconnect") { chat.disconnect() }
                    Button("Reconnect") { chat.disconnect(); chat.connect() }
                } else if chat.pairedMacs.selected != nil, !chat.repairRequired {
                    Button("Connect / Retry") { chat.connect() }
                }
            }
            Text("Point Guard runs on your Mac. Pair this iPad once; saved Macs reconnect securely through Tailscale.")
                .font(.callout).foregroundStyle(.secondary)
            if let error = chat.error { Text(error).foregroundStyle(.red).textSelection(.enabled) }
            if !chat.pairedMacs.machines.isEmpty {
                GroupBox("Macs") {
                    VStack(alignment: .leading, spacing: 10) {
                        ForEach(chat.pairedMacs.machines) { machine in
                            macRow(machine)
                        }
                    }.padding(8)
                }
            }
            GroupBox(chat.pairedMacs.machines.isEmpty ? "Pair with your Mac" : "Pair another Mac") {
                VStack(alignment: .leading, spacing: 10) {
                    HStack {
                        Text("On your Mac, choose Connect phone in Point Guard, then scan its QR code.")
                        Spacer()
                        Button("Scan QR code") { Task { await openScanner() } }
                            .disabled(requestingCamera || chat.pairing)
                    }
                    #if DEBUG
                    HStack {
                        TextField("Paste the Mac pairing code", text: $pastedCode).textFieldStyle(.roundedBorder)
                            .textInputAutocapitalization(.never).autocorrectionDisabled()
                        Button("Pair") {
                            guard let code = PiEnrollmentCode.parse(pastedCode) else {
                                chat.pairingError = "Paste a current Mac pairing code."; return
                            }
                            pastedCode = ""; chat.pair(code)
                        }.disabled(chat.pairing || pastedCode.isEmpty)
                    }
                    #endif
                    if chat.pairing {
                        HStack {
                            ProgressView().controlSize(.small)
                            Text("Pairing with your Mac…")
                            Spacer()
                            Button("Cancel pairing") { chat.cancelPairing() }
                        }
                    }
                    if let error = chat.pairingError { Text(error).foregroundStyle(.red).textSelection(.enabled) }
                }.padding(8)
            }
            if chat.connected, let grant = chat.terminalsGrant {
                GroupBox("Access") {
                    HStack {
                        Text("Agents and terminals")
                        Spacer()
                        Text(Self.grantName(grant)).foregroundStyle(.secondary)
                    }
                    .padding(8)
                    .help("Your Mac sets this iPad's access in Point Guard setup.")
                }
            }
        }
        .alert("Mac name", isPresented: Binding(get: { renaming != nil }, set: { if !$0 { renaming = nil } })) {
            TextField("Name", text: $machineName)
            Button("Save") {
                if let machine = renaming { chat.renameMac(machine.id, name: machineName) }
                renaming = nil
            }.disabled(PiPairedMac.normalizedName(machineName).isEmpty)
            Button("Cancel", role: .cancel) { renaming = nil }
        } message: {
            Text("Choose a name to recognize this Mac on your iPad. Up to 60 characters.")
        }
        .sheet(isPresented: $showScanner) {
            PointGuardPairingScanner { code in
                showScanner = false; chat.pair(code)
            }
        }
    }

    private func macRow(_ machine: PiPairedMac) -> some View {
        let selected = chat.pairedMacs.selectedId == machine.id
        return HStack {
            Image(systemName: "desktopcomputer").foregroundStyle(selected ? Color.accentColor : .secondary)
                .frame(width: 20)
            VStack(alignment: .leading, spacing: 2) {
                Text(machine.name).lineLimit(1)
                Text(machine.endpoint).font(.system(.caption, design: .monospaced)).foregroundStyle(.secondary)
                    .lineLimit(1).truncationMode(.middle).textSelection(.enabled)
            }
            Spacer()
            if selected {
                Text(chat.connected ? "Connected" : chat.repairRequired ? "Re-pair required" : "Selected")
                    .foregroundStyle(chat.repairRequired ? Color.red : .secondary)
            } else {
                Button("Connect") { chat.selectMac(machine.id) }
            }
            Button("Rename") { machineName = machine.name; renaming = machine }
            Button("Forget", role: .destructive) { chat.forgetMac(machine.id) }
                .help("Remove this Mac and its credential from this iPad. Revoke the iPad on the Mac too.")
        }
    }

    private static func grantName(_ grant: String) -> String {
        switch grant {
        case "view": return "View"
        case "control": return "Control"
        default: return "Off"
        }
    }

    private func openScanner() async {
        guard DataScannerViewController.isSupported else {
            chat.pairingError = "QR scanning is unavailable on this iPad."; return
        }
        requestingCamera = true
        defer { requestingCamera = false }
        guard await AVCaptureDevice.requestAccess(for: .video) else {
            chat.pairingError = "Allow camera access for PurePoint in iPad Settings to scan your Mac's QR code."; return
        }
        guard DataScannerViewController.isAvailable else {
            chat.pairingError = "The camera is unavailable. Try again."; return
        }
        chat.pairingError = nil
        showScanner = true
    }
}
#endif
