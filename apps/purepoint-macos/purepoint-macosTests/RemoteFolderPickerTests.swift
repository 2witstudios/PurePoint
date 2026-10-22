import Foundation
import Testing

@testable import PurePoint

// MARK: - Request encoding (both platforms)

struct FsBrowseRequestTests {
    @Test func givenFsBrowseShouldEncodeADirectoriesOnlyFsList() throws {
        let data = try JSONEncoder().encode(DaemonRequest.fsBrowse(path: "~", includeHidden: false))
        let object = try #require(try JSONSerialization.jsonObject(with: data) as? [String: Any])
        #expect(object["type"] as? String == "fs_list")
        #expect(object["path"] as? String == "~")
        #expect(object["include_hidden"] as? Bool == false)
        #expect(object["max_entries"] as? Int == 20_000)
        #expect(object["directories_only"] as? Bool == true)
        #expect(object["recursive"] == nil)
    }

    @Test func givenPlainFsListShouldNotAskForDirectoriesOnly() throws {
        let data = try JSONEncoder().encode(DaemonRequest.fsList(path: "/srv/app"))
        let object = try #require(try JSONSerialization.jsonObject(with: data) as? [String: Any])
        #expect(object["directories_only"] == nil)
    }
}

#if os(iOS)

// MARK: - Folder browser

/// A machine's folders, keyed by directory, served the way the daemon's browse mode answers.
@MainActor
private final class FakeMachineFolders {
    var tree: [String: [String]] = [
        "/home/me": ["code", ".config", "Documents"],
        "/home/me/code": ["app", "Zeta", "beta", "alpha2", "alpha10"],
        "/home/me/code/app": ["src"],
        "/home/me/code/app/src": [],
        "/home": ["me"],
        "/": ["home", "srv"],
        "/srv": [],
    ]
    var symlinks: Set<String> = ["/home/me/Documents"]
    var requests: [(path: String, includeHidden: Bool)] = []
    var failure: DaemonResponse?

    func list(_ machineId: UUID, _ path: String, _ includeHidden: Bool) async throws -> DaemonResponse {
        requests.append((path, includeHidden))
        if let failure { return failure }
        let resolved = path == "~" ? "/home/me" : path
        guard let names = tree[resolved] else {
            return .error(code: "not_found", message: "\(resolved) does not exist")
        }
        let entries = names.filter { includeHidden || !$0.hasPrefix(".") }.map { name in
            let child = resolved == "/" ? "/" + name : resolved + "/" + name
            return FsEntryPayload(
                name: name, path: child, isDir: true, isSymlink: symlinks.contains(child), size: 0,
                modifiedMs: 0, ignored: false)
        }
        return .fsListResult(path: resolved, entries: entries, truncated: false)
    }
}

@MainActor
struct RemoteFolderBrowserTests {
    private let machineId = UUID(uuidString: "6F1C2B3A-1111-4222-8333-944455556666")!

    private func browser() -> (RemoteFolderBrowser, FakeMachineFolders) {
        let folders = FakeMachineFolders()
        return (RemoteFolderBrowser(list: folders.list), folders)
    }

    @Test func givenAMachineShouldStartAtTheHomeFolderWithVisibleFoldersOnly() async {
        let (browser, folders) = browser()
        await browser.choose(machineId: machineId)
        #expect(folders.requests.first?.path == "~")
        #expect(folders.requests.first?.includeHidden == false)
        #expect(browser.path == "/home/me")
        #expect(browser.home == "/home/me")
        #expect(browser.folders.map(\.name) == ["code", "Documents"])
        #expect(browser.folders.last?.isSymlink == true)
        #expect(browser.selection == nil)
        #expect(browser.backStack.isEmpty)
    }

    @Test func givenFoldersShouldSortThemLikeFinder() async {
        let (browser, _) = browser()
        await browser.choose(machineId: machineId)
        await browser.navigate(to: "/home/me/code")
        #expect(browser.folders.map(\.name) == ["alpha2", "alpha10", "app", "beta", "Zeta"])
    }

    @Test func givenShowHiddenShouldListAgainKeepingTheSelection() async {
        let (browser, folders) = browser()
        await browser.choose(machineId: machineId)
        browser.selection = "/home/me/code"
        await browser.setShowHidden(true)
        #expect(folders.requests.last?.includeHidden == true)
        #expect(browser.folders.map(\.name) == [".config", "code", "Documents"])
        #expect(browser.selection == "/home/me/code")
        browser.selection = "/home/me/.config"
        await browser.setShowHidden(false)
        #expect(browser.selection == nil)
    }

    @Test func givenEnterUpBackAndForwardShouldKeepTheMacPanelHistory() async {
        let (browser, _) = browser()
        await browser.choose(machineId: machineId)
        await browser.enter(browser.folders[0])
        #expect(browser.path == "/home/me/code")
        #expect(browser.backStack == ["/home/me"])
        await browser.goUp()
        // The enclosing folder selects the one just left.
        #expect(browser.path == "/home/me")
        #expect(browser.selection == "/home/me/code")
        await browser.goBack()
        #expect(browser.path == "/home/me/code")
        #expect(browser.forwardStack == ["/home/me"])
        await browser.goForward()
        #expect(browser.path == "/home/me")
        #expect(browser.forwardStack.isEmpty)
        await browser.navigate(to: "/")
        #expect(browser.path == "/")
        // The path bar's jump selects the next folder down the path just left.
        #expect(browser.selection == "/home")
        await browser.goUp()
        #expect(browser.path == "/")
    }

    @Test func givenANewNavigationShouldClearForwardHistory() async {
        let (browser, _) = browser()
        await browser.choose(machineId: machineId)
        await browser.navigate(to: "/home/me/code")
        await browser.goBack()
        #expect(!browser.forwardStack.isEmpty)
        await browser.navigate(to: "/srv")
        #expect(browser.forwardStack.isEmpty)
        #expect(browser.backStack == ["/home/me"])
    }

    @Test func givenArrowKeysShouldMoveTheSelectionWithinTheList() async {
        let (browser, _) = browser()
        await browser.choose(machineId: machineId)
        await browser.navigate(to: "/home/me/code")
        browser.moveSelection(by: 1)
        #expect(browser.selectedFolder?.name == "alpha2")
        browser.moveSelection(by: -1)
        #expect(browser.selectedFolder?.name == "alpha2")
        browser.moveSelection(by: 10)
        #expect(browser.selectedFolder?.name == "Zeta")
        browser.selection = nil
        browser.moveSelection(by: -1)
        #expect(browser.selectedFolder?.name == "Zeta")
        browser.selectFirst()
        #expect(browser.selectedFolder?.name == "alpha2")
    }

    @Test func givenTypedCharactersShouldSelectByNamePrefix() async {
        let (browser, _) = browser()
        await browser.choose(machineId: machineId)
        await browser.navigate(to: "/home/me/code")
        let start = Date()
        browser.typeSelect("a", at: start)
        #expect(browser.selectedFolder?.name == "alpha2")
        browser.typeSelect("p", at: start.addingTimeInterval(0.3))
        #expect(browser.selectedFolder?.name == "app")
        // After a pause the prefix starts again.
        browser.typeSelect("z", at: start.addingTimeInterval(3))
        #expect(browser.selectedFolder?.name == "Zeta")
    }

    @Test func givenOpenShouldTargetTheSelectionElseTheFolderOnScreen() async {
        let (browser, _) = browser()
        await browser.choose(machineId: machineId)
        #expect(browser.openTarget == "/home/me")
        browser.selection = "/home/me/code"
        #expect(browser.openTarget == "/home/me/code")
    }

    @Test func givenRootHomeOrAnAncestorShouldRefuseToOpenThemAsProjects() async {
        let (browser, _) = browser()
        await browser.choose(machineId: machineId)
        #expect(browser.refusal(for: "/") != nil)
        #expect(browser.refusal(for: "/home") != nil)
        #expect(browser.refusal(for: "/home/me") != nil)
        #expect(browser.refusal(for: "/home/me/code") == nil)
        #expect(browser.refusal(for: "/home/meadow") == nil)
    }

    @Test func givenAPathShouldSplitItForThePathBar() async {
        let (browser, _) = browser()
        await browser.choose(machineId: machineId)
        await browser.navigate(to: "/home/me/code/app")
        #expect(browser.pathComponents.map(\.path) == ["/", "/home", "/home/me", "/home/me/code", "/home/me/code/app"])
        #expect(browser.pathComponents.map(\.name) == ["/", "home", "me", "code", "app"])
    }

    @Test func givenADaemonErrorShouldShowItAndKeepTheFolderOnScreen() async {
        let (browser, folders) = browser()
        await browser.choose(machineId: machineId)
        await browser.navigate(to: "/missing")
        #expect(browser.error == "/missing does not exist")
        #expect(browser.path == "/home/me")
        folders.failure = nil
        await browser.reload()
        #expect(browser.error == nil)
    }

    @Test func givenADaemonWithoutBrowsingShouldAskToUpdatePurePoint() async {
        let (browser, folders) = browser()
        folders.failure = .error(code: "path_not_allowed", message: "~ is not an absolute path")
        await browser.choose(machineId: machineId)
        #expect(browser.error?.contains("Update PurePoint") == true)
        #expect(browser.path == nil)
    }
}

// MARK: - Opening a folder on a machine

/// A daemon that answers each request with the next canned line, recording the requests.
private final class CannedDaemon: DaemonTransport, @unchecked Sendable {
    private let lock = NSLock()
    private var replies: [String]
    private(set) var requests: [[String: Any]] = []

    init(replies: [String]) { self.replies = replies }

    func openStream(cancellable: Bool) async throws -> any DaemonByteStream {
        CannedStream(daemon: self)
    }

    fileprivate func answer(_ request: Data) -> Data {
        lock.withLock {
            if let object = try? JSONSerialization.jsonObject(with: request) as? [String: Any] {
                requests.append(object)
            }
            return Data((replies.isEmpty ? #"{"type":"ok"}"# : replies.removeFirst()).utf8) + [0x0A]
        }
    }
}

private final class CannedStream: DaemonByteStream, @unchecked Sendable {
    private let daemon: CannedDaemon
    private let lock = NSLock()
    private var pending: [Data] = []
    private var buffer = Data()

    init(daemon: CannedDaemon) { self.daemon = daemon }

    func send(_ data: Data) async throws {
        lock.withLock {
            buffer.append(data)
            while let newline = buffer.firstIndex(of: 0x0A) {
                pending.append(daemon.answer(buffer[..<newline]))
                buffer = Data(buffer[buffer.index(after: newline)...])
            }
        }
    }

    func receive() async throws -> Data {
        try lock.withLock {
            guard !pending.isEmpty else { throw DaemonClientError.eof }
            return pending.removeFirst()
        }
    }

    func close() {}
}

@MainActor
struct MachineOpenProjectTests {
    private let machine = Machine(
        id: UUID(uuidString: "6F1C2B3A-1111-4222-8333-944455556666")!, name: "studio", sshHost: "me@studio")

    @Test func givenAFolderShouldRegisterItWithTheMachineDaemonAndMakeItActive() async throws {
        let appState = AppState(startProject: { _ in })
        let state = MachineState(machine: machine, appState: appState, registry: nil)
        let daemon = CannedDaemon(replies: [#"{"type":"init_result","created":true}"#])
        try await state.openProject(root: "/home/me/code/app", client: DaemonClient(transport: daemon))
        #expect(daemon.requests.first?["type"] as? String == "init")
        #expect(daemon.requests.first?["project_root"] as? String == "/home/me/code/app")
        #expect(appState.activeProjectRoot == ProjectKey(machineId: machine.id, root: "/home/me/code/app").id)
    }

    @Test func givenAHiddenFolderShouldShowItAgainWhenOpened() async throws {
        let state = MachineState(machine: machine, appState: nil, registry: nil)
        state.hideProject(root: "/home/me/code/app")
        #expect(state.hiddenProjectCount == 1)
        let daemon = CannedDaemon(replies: [#"{"type":"init_result","created":false}"#])
        try await state.openProject(root: "/home/me/code/app", client: DaemonClient(transport: daemon))
        #expect(state.hiddenProjectCount == 0)
    }

    @Test func givenTheDaemonRefusesShouldThrowItsMessage() async {
        let state = MachineState(machine: machine, appState: nil, registry: nil)
        let daemon = CannedDaemon(replies: [
            #"{"type":"error","code":"project_root_not_allowed","message":"/home/me is /, $HOME or an ancestor of $HOME"}"#
        ])
        await #expect(throws: MachineOpenProjectError(message: "/home/me is /, $HOME or an ancestor of $HOME")) {
            try await state.openProject(root: "/home/me", client: DaemonClient(transport: daemon))
        }
    }
}

@MainActor
struct MachineRemoveProjectTests {
    private let machine = Machine(
        id: UUID(uuidString: "6F1C2B3A-1111-4222-8333-944455556667")!, name: "studio", sshHost: "me@studio")

    @Test func givenAProjectShouldAskTheMachineDaemonToForgetItAndSuspendItsAgents() async throws {
        let state = MachineState(machine: machine, appState: nil, registry: nil)
        state.hideProject(root: "/home/me/code/app")
        let daemon = CannedDaemon(replies: [
            #"{"type":"project_removed","project_root":"/home/me/code/app","removed":true,"suspended":[]}"#
        ])
        try await state.removeProject(
            root: "/home/me/code/app", suspendAgents: true, client: DaemonClient(transport: daemon))
        #expect(daemon.requests.first?["type"] as? String == "remove_project")
        #expect(daemon.requests.first?["project_root"] as? String == "/home/me/code/app")
        #expect(daemon.requests.first?["suspend_agents"] as? Bool == true)
        // Removed from the machine, it no longer needs hiding on this device.
        #expect(state.hiddenProjectCount == 0)
    }

    @Test func givenTheDaemonRefusesShouldThrowItsMessage() async {
        let state = MachineState(machine: machine, appState: nil, registry: nil)
        let daemon = CannedDaemon(replies: [
            #"{"type":"error","code":"PROJECT_HAS_LIVE_AGENTS","message":"/srv/app has 1 live agent"}"#
        ])
        await #expect(throws: MachineOpenProjectError(message: "/srv/app has 1 live agent")) {
            try await state.removeProject(root: "/srv/app", suspendAgents: false, client: DaemonClient(transport: daemon))
        }
    }
}

#endif
