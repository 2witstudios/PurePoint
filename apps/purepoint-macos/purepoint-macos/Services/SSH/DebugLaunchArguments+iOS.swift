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
/// - `-PUDebugSelectAgent <agent id>` selects the workspace holding that agent, as if the
///   user clicked its sidebar row.
/// - `-PUDebugPointGuardPairingCode '<QR JSON>'` pairs Point Guard with that Mac enrollment
///   code, as if the user scanned it.
/// - `-PUDebugPointGuardSetup YES` opens Point Guard setup from the pane controls.
struct DebugLaunchArguments {
    var addMachine: (sshHost: String, name: String?)?
    var trustHostKey = false
    var selectAgent: String?
    var pointGuardPairingCode: String?
    var pointGuardSetup = false

    static let current = DebugLaunchArguments(defaults: .standard)

    init(defaults: UserDefaults, arguments: [String] = ProcessInfo.processInfo.arguments) {
        if let value = defaults.string(forKey: "PUDebugAddMachine"), !value.isEmpty {
            let parts = value.split(separator: ",", maxSplits: 1).map { $0.trimmingCharacters(in: .whitespaces) }
            addMachine = (parts[0], parts.count > 1 && !parts[1].isEmpty ? parts[1] : nil)
        }
        trustHostKey = defaults.bool(forKey: "PUDebugTrustHostKey")
        if let value = defaults.string(forKey: "PUDebugSelectAgent"), !value.isEmpty {
            selectAgent = value
        }
        // Read raw: the defaults argument domain parses a JSON object as a property list.
        if let index = arguments.firstIndex(of: "-PUDebugPointGuardPairingCode"), arguments.indices.contains(index + 1),
            !arguments[index + 1].isEmpty
        {
            pointGuardPairingCode = arguments[index + 1]
        }
        pointGuardSetup = defaults.bool(forKey: "PUDebugPointGuardSetup")
    }

    /// The workspace holding the `-PUDebugSelectAgent` agent, once a machine's layout lists it.
    /// Gives up after a minute.
    @MainActor
    func selectAgentWorkspace(registry: WorkspaceRegistry) async -> SidebarSelection? {
        guard let selectAgent else { return nil }
        for _ in 0..<120 {
            if let id = registry.workspaceId(forAgent: selectAgent) { return .workspace(id) }
            do { try await Task.sleep(for: .milliseconds(500)) } catch { return nil }
        }
        print("[SSH] -PUDebugSelectAgent: no workspace holds \(selectAgent)")
        return nil
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
