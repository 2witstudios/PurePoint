#if os(iOS)
import Foundation

/// App-start wiring for machines on iPad: the SSH transport provider and the device identity.
@MainActor
enum SSHMachineSupport {
    private static var launched = false

    /// Install the transport provider and make sure the device has its identity, generating it
    /// on first launch (REQ-IPAD-020). Safe to call more than once.
    static func launch() {
        guard !launched else { return }
        launched = true
        SSHMachineSessions.installTransportProvider()
        do {
            let identity = try SSHIdentityStore.shared.identity()
            #if DEBUG
            DebugLaunchArguments.writePublicKey(identity.authorizedKeyLine)
            #else
            _ = identity
            #endif
        } catch {
            // Shown again where the key is needed (Add Machine, Settings); connects fail with it.
            print("[SSH] Could not load the device identity: \(error.localizedDescription)")
        }
    }

    /// After the machines loaded: run the DEBUG add-machine launch argument, if given.
    static func machinesLoaded(appState: AppState) {
        #if DEBUG
        DebugLaunchArguments.current.runAddMachine(appState: appState)
        #endif
    }
}
#endif
