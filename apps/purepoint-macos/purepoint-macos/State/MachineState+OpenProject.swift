import Foundation

extension MachineState {
    /// Open a folder on this machine as a project: register it with the machine's daemon
    /// (`init`, the request the Mac sends when it opens a local project), then re-read the
    /// daemon's projects so it joins the sidebar under this machine. Throws the daemon's
    /// message when it refuses the folder. `client` defaults to the machine's daemon.
    func openProject(root: String, client: DaemonClient? = nil) async throws {
        let client = client ?? endpoint.makeClient()
        var attempt = 0
        while true {
            let response = try await client.send(.initProject(projectRoot: root))
            switch response {
            case .initResult:
                break
            // At its connection limit: transient, so back off and retry, as the Mac's open does.
            case .error(let code, _) where code == "BUSY" && attempt < 4:
                attempt += 1
                try await Task.sleep(for: .seconds(attempt))
                continue
            case .error(_, let message):
                throw MachineOpenProjectError(message: message)
            default:
                throw MachineOpenProjectError(message: "\(machine.name) did not register \(root).")
            }
            break
        }
        // A folder the user hid earlier comes back when they open it again.
        unhideProject(root: root)
        // Otherwise the project arrives with the inventory read when the daemon is back.
        if state.isDaemonRunning { await refreshProjects() }
        appState?.initializeActiveProjectIfNeeded(root: ProjectKey(machineId: machine.id, root: root).id)
    }
}

struct MachineOpenProjectError: LocalizedError, Equatable {
    let message: String
    var errorDescription: String? { message }
}
