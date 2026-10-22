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

extension MachineState {
    /// Remove a project from the machine for every device (`remove_project`). Its folder and
    /// `.pu/` are kept, so opening it again brings it back as it was. The daemon refuses while
    /// agents run unless `suspendAgents`, which suspends them so they can be resumed once it is added again.
    func removeProject(root: String, suspendAgents: Bool, client: DaemonClient? = nil) async throws {
        try await (client ?? endpoint.makeClient()).removeProject(
            root: root, suspendAgents: suspendAgents, daemonName: machine.name)
        // Nothing left to hide: a later open shows it again everywhere.
        unhideProject(root: root)
        if state.isDaemonRunning { await refreshProjects() }
    }
}

extension DaemonClient {
    /// Ask this daemon to forget a project (`remove_project`), throwing its refusal.
    func removeProject(root: String, suspendAgents: Bool, daemonName: String) async throws {
        switch try await send(.removeProject(projectRoot: root, suspendAgents: suspendAgents)) {
        case .projectRemoved:
            return
        case .error(_, let message):
            throw MachineOpenProjectError(message: message)
        default:
            throw MachineOpenProjectError(message: "\(daemonName) did not remove \(root).")
        }
    }
}

/// A project action the machine's daemon refused, with its message.
struct MachineOpenProjectError: LocalizedError, Equatable {
    let message: String
    var errorDescription: String? { message }
}
