#if os(iOS)
import Foundation
import Observation

#if DEBUG
/// Launch arguments that let scripts connect a simulator to a test machine without touching
/// the UI (`docs/product/ipad-app.md`, Debug launch arguments). Each drives the same code the
/// UI does. Debug builds only.
///
/// - Every launch writes the device public key to
///   `<data container>/Documents/purepoint-debug/id_ed25519.pub`.
/// - `-PUDebugAddMachine user@host[:port][,name]` submits Add Machine with those fields.
/// - `-PUDebugTrustHostKey YES` confirms a first-use host key as if the user tapped Trust.
struct DebugLaunchArguments {
    var addMachine: (sshHost: String, name: String?)?
    var trustHostKey = false

    static let current = DebugLaunchArguments(defaults: .standard)

    init(defaults: UserDefaults) {
        if let value = defaults.string(forKey: "PUDebugAddMachine"), !value.isEmpty {
            let parts = value.split(separator: ",", maxSplits: 1).map { $0.trimmingCharacters(in: .whitespaces) }
            addMachine = (parts[0], parts.count > 1 && !parts[1].isEmpty ? parts[1] : nil)
        }
        trustHostKey = defaults.bool(forKey: "PUDebugTrustHostKey")
    }

    static var publicKeyURL: URL {
        FileManager.default.urls(for: .documentDirectory, in: .userDomainMask)[0]
            .appendingPathComponent("purepoint-debug/id_ed25519.pub")
    }

    static func writePublicKey(_ line: String) {
        let url = publicKeyURL
        try? FileManager.default.createDirectory(at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
        try? Data((line + "\n").utf8).write(to: url, options: .atomic)
    }

    /// Submit Add Machine unless a machine with that destination is already registered.
    @MainActor
    func runAddMachine(appState: AppState) {
        guard let addMachine else { return }
        if appState.machines.contains(where: { $0.machine.sshHost == SSHDestination(sshHost: addMachine.sshHost)?.sshHost }) {
            return
        }
        let flow = AddMachineFlow()
        guard flow.fill(sshHost: addMachine.sshHost, name: addMachine.name) else {
            print("[SSH] -PUDebugAddMachine: expected user@host[:port][,name], got \(addMachine.sshHost)")
            return
        }
        DebugAddMachinePresenter.shared.flow = flow
        Task {
            await flow.submit(appState: appState)
            print("[SSH] -PUDebugAddMachine \(flow.name): \(flow.step)\(flow.error.map { " — \($0)" } ?? "")")
        }
    }
}

/// Hands the launch argument's flow to the sidebar footer, which shows the sheet for it.
@Observable
@MainActor
final class DebugAddMachinePresenter {
    static let shared = DebugAddMachinePresenter()
    var flow: AddMachineFlow?
}
#endif
#endif
