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
/// - `-PUDebugOpen <route>` opens a screen through the navigation the UI uses (`DebugOpenRoute`).
struct DebugLaunchArguments {
    var addMachine: (sshHost: String, name: String?)?
    var trustHostKey = false
    var selectAgent: String?
    var pointGuardPairingCode: String?
    var pointGuardSetup = false
    var open: DebugOpenRoute?

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
        if let value = defaults.string(forKey: "PUDebugOpen"), !value.isEmpty {
            open = DebugOpenRoute(value)
            if open == nil { print("[Debug] -PUDebugOpen: unknown route \(value)") }
        }
    }

    /// Opens the `-PUDebugOpen` route once the machine's projects it names have loaded, the way
    /// the UI would: by setting the sidebar selection, or the flag the button sets. Gives up
    /// after a minute.
    @MainActor
    func runOpen(appState: AppState, select: (SidebarSelection) -> Void) async {
        guard let open else { return }
        for _ in 0..<120 {
            if let action = open.resolve(appState: appState) {
                switch action {
                case .select(let selection): select(selection)
                case .run(let body): body()
                }
                return
            }
            do { try await Task.sleep(for: .milliseconds(500)) } catch { return }
        }
        print("[Debug] -PUDebugOpen: \(open) never resolved")
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

/// A screen `-PUDebugOpen` can open:
///
/// | Route | Opens |
/// |---|---|
/// | `dashboard`, `agents`, `schedule` | the sidebar nav row |
/// | `settings[/<section>]` | Settings, at `general`, `pointGuard`, `agents`, `hotkeys` or `display` |
/// | `project/<name>` | the project's detail (first project with that name) |
///
/// No route selects a workspace: that attaches its terminals, which claims their size for
/// whoever else is watching. `-PUDebugSelectAgent` names the agent explicitly instead.
/// | `worktree/<project>/<branch>` | the worktree's detail |
/// | `channel/<name>` | the project's channel |
/// | `add-machine` | the Add Machine sheet |
/// | `open-project` | the Open Project remote folder picker |
enum DebugOpenRoute: Equatable, CustomStringConvertible {
    case nav(SidebarNavItem)
    case settings(SettingsSection?)
    case project(String)
    case worktree(project: String, branch: String)
    case channel(String)
    case addMachine
    case openProject

    enum Action {
        case select(SidebarSelection)
        case run(() -> Void)
    }

    init?(_ value: String) {
        let parts = value.split(separator: "/", maxSplits: 2).map(String.init)
        switch (parts.first, parts.count) {
        case ("dashboard", 1): self = .nav(.dashboard)
        case ("agents", 1): self = .nav(.agents)
        case ("schedule", 1): self = .nav(.schedule)
        case ("settings", 1): self = .settings(nil)
        case ("settings", 2):
            guard let section = SettingsSection(rawValue: parts[1]) else { return nil }
            self = .settings(section)
        case ("project", 2): self = .project(parts[1])
        case ("worktree", 3): self = .worktree(project: parts[1], branch: parts[2])
        case ("channel", 2): self = .channel(parts[1])
        case ("add-machine", 1): self = .addMachine
        case ("open-project", 1): self = .openProject
        default: return nil
        }
    }

    var description: String { "\(self)" }

    /// The settings section to show first, when this route opens Settings at one.
    var settingsSection: SettingsSection? {
        if case .settings(let section) = self { return section }
        return nil
    }

    /// What opening the route does now, or nil while what it names has not loaded.
    @MainActor
    func resolve(appState: AppState) -> Action? {
        func project(_ name: String) -> ProjectState? {
            appState.remoteProjects.first { $0.projectName == name }
        }
        switch self {
        case .nav(let item):
            return .select(.nav(item))
        case .settings:
            return .run { appState.showSettings = true }
        case .project(let name):
            return project(name).map { .select(.project($0.projectId)) }
        case .worktree(let name, let branch):
            guard let worktree = project(name)?.worktrees.first(where: { $0.branch == branch || $0.name == branch })
            else { return nil }
            return .select(.worktree(worktree.id))
        case .channel(let name):
            return project(name).map { .select(.channel($0.projectId)) }
        case .addMachine:
            return .run { DebugAddMachinePresenter.shared.showEmpty = true }
        case .openProject:
            guard let open = AppLifecycleHooks.openProject, !appState.machines.isEmpty else { return nil }
            return .run { open() }
        }
    }
}

/// Hands the launch argument's flow to the sidebar footer, which shows the sheet for it.
@Observable
@MainActor
final class DebugAddMachinePresenter {
    static let shared = DebugAddMachinePresenter()
    var flow: AddMachineFlow?
    /// `-PUDebugOpen add-machine`: show the sheet empty, as the footer button does.
    var showEmpty = false
}
#endif
#endif
