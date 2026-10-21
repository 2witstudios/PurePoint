import Foundation
import Testing

@testable import PurePoint

/// Encode a request the way DaemonClient sends it and read it back as a JSON object.
private func json(_ request: DaemonRequest) throws -> [String: Any] {
    let data = try JSONEncoder().encode(request)
    return try #require(try JSONSerialization.jsonObject(with: data) as? [String: Any])
}

private func decode(_ text: String) -> DaemonResponse {
    DaemonClient.parse(Data(text.utf8))
}

// MARK: - Project identity

struct ProjectKeyTests {
    private let machineId = UUID(uuidString: "6F1C2B3A-1111-4222-8333-944455556666")!

    @Test func givenLocalKeyShouldSerializeToTheBareRoot() {
        let key = ProjectKey(root: "/Users/me/code/app")
        #expect(key.id == "/Users/me/code/app")
        #expect(!key.isRemote)
        // Persisted Point Guard ids, channel defaults and registry keys keep their old strings.
        #expect(Workspace.pointGuardId(projectRoot: key.id) == "ws-point-guard:/Users/me/code/app")
    }

    @Test func givenRemoteKeyShouldUseTheMachinePrefixWithALowercaseId() {
        let key = ProjectKey(machineId: machineId, root: "/home/me/app")
        #expect(key.id == "machine:6f1c2b3a-1111-4222-8333-944455556666:/home/me/app")
        #expect(key.isRemote)
    }

    @Test func givenKeyStringsShouldRoundTrip() {
        let remote = ProjectKey(machineId: machineId, root: "/srv/with:colon/app")
        #expect(ProjectKey(id: remote.id) == remote)
        let local = ProjectKey(root: "/tmp/x")
        #expect(ProjectKey(id: local.id) == local)
    }

    @Test func givenLegacyOrMalformedStringsShouldParseAsLocalRoots() {
        #expect(ProjectKey(id: "/Users/me/app") == ProjectKey(root: "/Users/me/app"))
        #expect(ProjectKey(id: "machine:not-a-uuid:/x") == ProjectKey(root: "machine:not-a-uuid:/x"))
    }

    @Test func givenSamePathOnTwoMachinesShouldBeDistinctProjects() {
        let other = UUID()
        let keys: Set<ProjectKey> = [
            ProjectKey(root: "/app"), ProjectKey(machineId: machineId, root: "/app"),
            ProjectKey(machineId: other, root: "/app"),
        ]
        #expect(keys.count == 3)
        #expect(Set(keys.map(\.id)).count == 3)
    }

    @Test func givenMachineShouldForwardToItsPrivateSocketDirectory() {
        let endpoint = DaemonEndpoint.machine(machineId)
        #expect(endpoint.socketPath.hasSuffix("/.pu/machines/6f1c2b3a-1111-4222-8333-944455556666/daemon.sock"))
        #expect(DaemonEndpoint.local.socketPath.hasSuffix("/.pu/daemon.sock"))
        #expect(DaemonEndpoint.local.isLocal && !endpoint.isLocal)
    }
}

// MARK: - Machine registry

struct MachineRegistryTests {
    @Test func givenCLIRegistryShouldDecode() throws {
        let text = """
            {
              "version": 1,
              "machines": [
                { "id": "6f1c2b3a-1111-4222-8333-944455556666", "name": "studio", "ssh_host": "studio-mac" }
              ]
            }
            """
        let machines = try MachineRegistryStore.decode(Data(text.utf8))
        #expect(machines.count == 1)
        #expect(machines[0].id == UUID(uuidString: "6F1C2B3A-1111-4222-8333-944455556666"))
        #expect(machines[0].name == "studio")
        #expect(machines[0].sshHost == "studio-mac")
    }

    @Test func givenMachinesShouldEncodeTheSharedSnakeCaseFormat() throws {
        let machine = Machine(
            id: UUID(uuidString: "6F1C2B3A-1111-4222-8333-944455556666")!, name: "studio", sshHost: "me@host:2222")
        let data = try MachineRegistryStore.encode([machine])
        let object = try #require(try JSONSerialization.jsonObject(with: data) as? [String: Any])
        #expect(object["version"] as? Int == 1)
        let entry = try #require((object["machines"] as? [[String: Any]])?.first)
        #expect(entry["id"] as? String == "6f1c2b3a-1111-4222-8333-944455556666")
        #expect(entry["name"] as? String == "studio")
        #expect(entry["ssh_host"] as? String == "me@host:2222")
        #expect(entry["sshHost"] == nil)
        #expect(try MachineRegistryStore.decode(data) == [machine])
    }

    @Test func givenUnknownVersionShouldRefuseToLoad() {
        let text = #"{ "version": 2, "machines": [] }"#
        #expect(throws: MachineRegistryError.unsupportedVersion(2)) {
            try MachineRegistryStore.decode(Data(text.utf8))
        }
    }

    @Test func givenUpdateShouldWriteAPrivateFileAndRejectDuplicateNamesInAnyCase() throws {
        let directory = NSTemporaryDirectory() + "pp-machines-" + UUID().uuidString
        defer { try? FileManager.default.removeItem(atPath: directory) }
        let path = directory + "/machines.json"
        #expect(try MachineRegistryStore.load(path: path).isEmpty)

        let added = try MachineRegistryStore.update(path: path) { machines in
            try MachineRegistryStore.add(name: "Studio", sshHost: "studio-mac", to: &machines)
        }
        #expect(try MachineRegistryStore.load(path: path) == [added])
        let mode = try FileManager.default.attributesOfItem(atPath: path)[.posixPermissions] as? NSNumber
        #expect(mode?.intValue == 0o600)

        #expect(throws: MachineRegistryError.duplicateName("Studio")) {
            try MachineRegistryStore.update(path: path) { machines in
                try MachineRegistryStore.add(name: "studio", sshHost: "other", to: &machines)
            }
        }
        #expect(try MachineRegistryStore.load(path: path).count == 1)
    }

    @Test func givenInvalidNamesOrHostsShouldReject() {
        for name in ["", "-x", ".hidden", "has space", "slash/name", "colon:x", String(repeating: "a", count: 65)] {
            #expect(throws: MachineRegistryError.self) { try MachineRegistryStore.validate(name: name) }
        }
        for name in ["studio", "Mac-2", "build_box.local", "1"] {
            #expect(throws: Never.self) { try MachineRegistryStore.validate(name: name) }
        }
        for host in ["", "-oProxyCommand=x", "a b", "a\nb"] {
            #expect(throws: MachineRegistryError.self) { try MachineRegistryStore.validate(sshHost: host) }
        }
        for host in ["studio-mac", "me@10.0.0.2", "me@host:2222"] {
            #expect(throws: Never.self) { try MachineRegistryStore.validate(sshHost: host) }
        }
    }
}

// MARK: - SSH

struct MachineSSHTests {
    @Test func givenSSHConfigShouldOfferConcreteHostAliasesOnly() {
        let config = """
            # comment
            Host studio studio-alt
              HostName 10.0.0.2
            Host *.internal
            host=build
            Host !bad gpu? "quoted"
            Match host foo
            Host studio
            """
        #expect(SSHConfig.hostAliases(from: config) == ["studio", "studio-alt", "build", "quoted"])
    }

    @Test func givenHostShouldResolveHomeWithBatchMode() {
        #expect(
            MachineSSH.resolveHomeArguments(sshHost: "studio")
                == [
                    "-o", "BatchMode=yes", "-o", "ControlMaster=no", "-o", "ControlPath=none", "studio",
                    "printf %s \"$HOME\"",
                ])
    }

    @Test func givenRemoteHomeWithSpacesShouldForwardAsOneArgument() {
        let arguments = MachineSSH.forwardArguments(
            localSocket: "/Users/me/.pu/machines/abc/daemon.sock", remoteHome: "/Users/Jane Doe", sshHost: "studio")
        #expect(
            arguments == [
                "-N", "-T",
                "-o", "BatchMode=yes",
                "-o", "ExitOnForwardFailure=yes",
                "-o", "ServerAliveInterval=15",
                "-o", "ServerAliveCountMax=3",
                "-o", "StreamLocalBindUnlink=yes",
                "-o", "ControlMaster=no",
                "-o", "ControlPath=none",
                "-L", "/Users/me/.pu/machines/abc/daemon.sock:/Users/Jane Doe/.pu/daemon.sock",
                "studio",
            ])
    }

    @Test func givenStartDaemonShouldPutInstalledLocationsOnPath() {
        #expect(
            MachineSSH.startDaemonArguments(sshHost: "studio")
                == [
                    "-o", "ControlMaster=no", "-o", "ControlPath=none", "studio",
                    #"/bin/sh -c 'PATH="$HOME/.pu/bin:$HOME/.cargo/bin:$PATH"; export PATH; exec pu health'"#,
                ])
    }

    @Test func givenHostWithPortShouldUseTheSSHURIForm() {
        #expect(MachineSSH.destination("jane@studio:2222") == "ssh://jane@studio:2222")
        #expect(MachineSSH.destination("studio:22") == "ssh://studio:22")
        #expect(MachineSSH.destination("studio") == "studio")
        #expect(MachineSSH.destination("jane@studio") == "jane@studio")
        #expect(MachineSSH.destination("ssh://jane@studio:2222") == "ssh://jane@studio:2222")
        #expect(MachineSSH.destination("studio:") == "studio:")
        #expect(MachineSSH.destination(":22") == ":22")
        #expect(MachineSSH.destination("studio:ssh") == "studio:ssh")
        #expect(MachineSSH.destination("fe80::1:22") == "fe80::1:22")
    }

    @Test func givenHostWithPortEveryCommandShouldUseTheDestination() {
        let host = "jane@studio:2222"
        #expect(MachineSSH.resolveHomeArguments(sshHost: host).dropLast().last == "ssh://jane@studio:2222")
        #expect(
            MachineSSH.forwardArguments(localSocket: "/l", remoteHome: "/h", sshHost: host).last
                == "ssh://jane@studio:2222")
        #expect(MachineSSH.startDaemonArguments(sshHost: host).dropLast().last == "ssh://jane@studio:2222")
    }

    @Test func givenRecordedPidShouldOnlyMatchThisMachinesSSHForward() {
        let socket = "/Users/me/.pu/machines/abc/daemon.sock"
        let forward = MachineSSH.forwardArguments(localSocket: socket, remoteHome: "/home/me", sshHost: "studio")
        #expect(
            ForwardPidFile.isForward(executablePath: "/usr/bin/ssh", arguments: ["ssh"] + forward, localSocket: socket))
        // Reused by the user's own ssh session, another machine's forward, or another program.
        #expect(
            !ForwardPidFile.isForward(
                executablePath: "/usr/bin/ssh", arguments: ["ssh", "studio"], localSocket: socket))
        #expect(
            !ForwardPidFile.isForward(
                executablePath: "/usr/bin/ssh", arguments: ["ssh"] + forward,
                localSocket: "/Users/me/.pu/machines/xyz/daemon.sock"))
        #expect(
            !ForwardPidFile.isForward(executablePath: "/bin/zsh", arguments: ["zsh"] + forward, localSocket: socket))
        #expect(!ForwardPidFile.isForward(executablePath: nil, arguments: nil, localSocket: socket))
    }

    @Test func givenForwardLockShouldBeExclusiveAcrossHolders() throws {
        let id = UUID()
        _ = try MachinePaths.ensureDirectory(machineId: id)
        defer { try? FileManager.default.removeItem(atPath: MachinePaths.directory(machineId: id)) }
        let first = ForwardLock(machineId: id)
        let second = ForwardLock(machineId: id)
        #expect(first.tryAcquire())
        #expect(first.tryAcquire())
        // flock locks belong to the open file description, so a second holder in this
        // process stands in for another app instance.
        #expect(!second.tryAcquire())
        first.release()
        #expect(second.tryAcquire())
        second.release()
    }

    @Test func givenSSHExitShouldReturnWithoutWaitingForLingeringPipes() async {
        // A child that outlives the command keeps stdout open, as a ControlPersist master can.
        let started = Date()
        // The lingering child lives 300 s; returning well before it proves the run did not
        // wait for its pipes. The bound is generous because the suite runs in parallel.
        let output = await MachineSSH.run(["-c", "(sleep 300 &) ; echo done"], timeout: 240, executable: "/bin/sh")
        #expect(output.status == 0)
        #expect(output.stdout.contains("done"))
        #expect(Date().timeIntervalSince(started) < 120)
    }

    @Test func givenTimeoutShouldEndTheCommand() async {
        let started = Date()
        // A 300 s command with a 1 s deadline: returning well before it ends proves the kill.
        let output = await MachineSSH.run(["300"], timeout: 1, executable: "/bin/sleep")
        #expect(output.status != 0)
        #expect(Date().timeIntervalSince(started) < 120)
    }

    @Test func givenThisProcessShouldReadItsExecutableAndArguments() {
        let pid = getpid()
        #expect(ForwardPidFile.executablePath(of: pid) != nil)
        #expect(ForwardPidFile.arguments(of: pid)?.isEmpty == false)
    }
}

// MARK: - Daemon health

struct HealthProbeTests {
    @Test func givenOneOrTwoFailedProbesShouldKeepTheLastState() {
        var tracker = HealthProbeTracker()
        #expect(tracker.record(.running) == .running)
        #expect(tracker.record(.failed) == nil)
        #expect(tracker.record(.failed) == nil)
        #expect(tracker.record(.running) == .running)
        #expect(tracker.record(.failed) == nil)
        #expect(tracker.record(.failed) == nil)
        #expect(tracker.record(.failed) == .stopped)
    }

    @Test func givenBusyShouldNeitherReportRunningNorCountAsFailure() {
        var tracker = HealthProbeTracker()
        #expect(tracker.record(.busy) == nil)
        #expect(tracker.record(.failed) == nil)
        #expect(tracker.record(.busy) == nil)
        #expect(tracker.record(.failed) == nil)
        #expect(tracker.consecutiveFailures == 2)
        #expect(tracker.record(.incompatible(7)) == .incompatible(7))
    }

    @Test func givenHealthRepliesShouldClassify() {
        #expect(MachineConnection.classify(.error(code: "BUSY", message: "full")) == .busy)
        #expect(MachineConnection.classify(.error(code: "IO", message: "x")) == .failed)
        #expect(MachineConnection.classify(nil) == .failed)
        #expect(
            MachineConnection.classify(
                .healthReport(pid: 1, uptimeSeconds: 1, protocolVersion: DaemonProtocol.version, agentCount: 0))
                == .running)
        #expect(
            MachineConnection.classify(.healthReport(pid: 1, uptimeSeconds: 1, protocolVersion: 2, agentCount: 0))
                == .incompatible(2))
    }
}

// MARK: - Daemon file and git requests (protocol 9)

struct DaemonProjectHostRequestTests {
    @Test func givenProtocolShouldBeCurrentVersion() {
        #expect(DaemonProtocol.version == 11)
    }

    @Test func givenInventoryShouldAskForEveryRegisteredProject() throws {
        let object = try json(.inventory(kind: "projects"))
        #expect(object["type"] as? String == "inventory")
        #expect(object["kind"] as? String == "projects")
        #expect(object["project_root"] == nil)
    }

    @Test func givenListShouldEncodeFsList() throws {
        let object = try json(DaemonProjectHost.listRequest(directory: "/srv/app/src"))
        #expect(object["type"] as? String == "fs_list")
        #expect(object["path"] as? String == "/srv/app/src")
        #expect(object["recursive"] as? Bool == false)
        #expect(object["include_hidden"] as? Bool == true)
        #expect(object["max_entries"] as? Int == 20_000)
        let recursive = try json(DaemonProjectHost.listRecursiveRequest(root: "/srv/app", limit: 500))
        #expect(recursive["recursive"] as? Bool == true)
        #expect(recursive["max_entries"] as? Int == 500)
    }

    @Test func givenStatReadAndDeleteShouldEncodeTheirPaths() throws {
        let stat = try json(.fsStat(path: "/srv/app/a.txt"))
        #expect(stat["type"] as? String == "fs_stat")
        #expect(stat["path"] as? String == "/srv/app/a.txt")
        let read = try json(DaemonProjectHost.readRequest(path: "/srv/app/a.txt", maxBytes: 1_048_576))
        #expect(read["type"] as? String == "fs_read")
        #expect(read["max_bytes"] as? Int == 1_048_576)
        let delete = try json(.fsDelete(path: "/srv/app/a.txt"))
        #expect(delete["type"] as? String == "fs_delete")
        #expect(delete["path"] as? String == "/srv/app/a.txt")
    }

    @Test func givenWriteShouldSendBase64AndTheExpectedModificationTime() throws {
        let object = try json(
            DaemonProjectHost.writeRequest(
                path: "/srv/app/a.txt", data: Data("héllo\n".utf8), expectedModifiedMs: 1_700_000_000_123,
                createDirectories: false))
        #expect(object["type"] as? String == "fs_write")
        #expect(object["data_base64"] as? String == Data("héllo\n".utf8).base64EncodedString())
        #expect((object["expected_modified_ms"] as? NSNumber)?.int64Value == 1_700_000_000_123)
        #expect(object["create_dirs"] as? Bool == false)

        let unconditional = try json(
            DaemonProjectHost.writeRequest(path: "/x", data: Data(), expectedModifiedMs: nil, createDirectories: true))
        #expect(unconditional["expected_modified_ms"] == nil)
        #expect(unconditional["create_dirs"] as? Bool == true)
    }

    @Test func givenSubscribeShouldWatchRecursively() throws {
        let object = try json(.subscribeFs(root: "/srv/app"))
        #expect(object["type"] as? String == "subscribe_fs")
        #expect(object["root"] as? String == "/srv/app")
        #expect(object["recursive"] as? Bool == true)
    }

    @Test func givenGitCommandShouldRunTheProgramWithoutAShell() throws {
        let command = HostCommand(
            program: "git", executablePath: "/usr/bin/git", args: ["--literal-pathspecs", "status", "-z"],
            cwd: "/srv/app", stdin: Data("a\nb".utf8), timeoutSeconds: 15)
        let object = try json(DaemonProjectHost.execRequest(command))
        #expect(object["type"] as? String == "git_exec")
        #expect(object["cwd"] as? String == "/srv/app")
        #expect(object["program"] as? String == "git")
        #expect(object["args"] as? [String] == ["--literal-pathspecs", "status", "-z"])
        #expect(object["stdin_base64"] as? String == Data("a\nb".utf8).base64EncodedString())
        #expect(object["timeout_ms"] as? Int == 15_000)

        let gh = HostCommand(program: "gh", executablePath: "gh", args: ["pr", "list"], cwd: "/srv/app")
        let ghObject = try json(DaemonProjectHost.execRequest(gh))
        #expect(ghObject["program"] as? String == "gh")
        #expect(ghObject["stdin_base64"] == nil)
        #expect(ghObject["timeout_ms"] as? Int == 30_000)
    }

    @Test func givenFileResponsesShouldDecode() {
        let list = decode(
            #"{"type":"fs_list_result","path":"/private/srv/app","truncated":false,"entries":[{"name":"a.txt","path":"/private/srv/app/a.txt","is_dir":false,"is_symlink":false,"size":3,"modified_ms":1700000000000,"ignored":true}]}"#
        )
        guard case .fsListResult(let path, let entries, let truncated) = list else {
            Issue.record("unexpected \(list)")
            return
        }
        #expect(path == "/private/srv/app")
        #expect(!truncated)
        #expect(entries.first?.ignored == true)
        #expect(entries.first?.modifiedMs == 1_700_000_000_000)

        let read = decode(
            #"{"type":"fs_read_result","path":"/a","data_base64":"aGk=","size":2,"modified_ms":5,"truncated":false}"#)
        guard case .fsReadResult(_, let data, let size, let modified, _) = read else {
            Issue.record("unexpected \(read)")
            return
        }
        #expect(String(decoding: data, as: UTF8.self) == "hi")
        #expect(size == 2 && modified == 5)

        let stat = decode(
            #"{"type":"fs_stat_result","path":"/a","exists":false,"is_dir":false,"size":0,"modified_ms":0}"#)
        guard case .fsStatResult(let payload) = stat else {
            Issue.record("unexpected \(stat)")
            return
        }
        #expect(!payload.exists)

        let exec = decode(
            #"{"type":"git_exec_result","exit_code":1,"stdout_base64":"b3V0","stderr_base64":"ZXJy","truncated":false}"#
        )
        guard case .gitExecResult(let code, let stdout, let stderr, _) = exec else {
            Issue.record("unexpected \(exec)")
            return
        }
        #expect(code == 1)
        #expect(String(decoding: stdout, as: UTF8.self) == "out")
        #expect(String(decoding: stderr, as: UTF8.self) == "err")

        guard case .fsEvent(let root, let paths) = decode(#"{"type":"fs_event","root":"/a","paths":["/a/b"]}"#) else {
            Issue.record("fs_event did not decode")
            return
        }
        #expect(root == "/a" && paths == ["/a/b"])
    }

    @Test func givenInventoryShouldDecodeProjects() {
        let response = decode(
            #"{"type":"inventory","observed_at":"2026-10-12T00:00:00Z","project_root":null,"complete":true,"summary":{"projects":1,"worktrees":0,"agents":0,"running":0,"suspended":0,"broken":0,"unknown":0,"running_ai_agents":0,"running_terminals":0},"projects":[{"project_root":"/srv/app","name":"app","available":true,"error":null,"counts":{"projects":1,"worktrees":0,"agents":0,"running":0,"suspended":0,"broken":0,"unknown":0,"running_ai_agents":0,"running_terminals":0}}],"agents":[],"worktrees":[]}"#
        )
        guard case .inventory(let report) = response else {
            Issue.record("unexpected \(response)")
            return
        }
        #expect(report.projects.map(\.projectRoot) == ["/srv/app"])
        #expect(report.projects.first?.available == true)
    }

    @Test func givenCanonicalListingPathsShouldRebaseOntoTheRequestedRoot() {
        #expect(
            DaemonProjectHost.rebase(
                "/private/var/app/src/a.swift", canonicalRoot: "/private/var/app", requestedRoot: "/var/app")
                == "/var/app/src/a.swift")
        #expect(
            DaemonProjectHost.rebase("/private/var/app", canonicalRoot: "/private/var/app", requestedRoot: "/var/app")
                == "/var/app")
        #expect(
            DaemonProjectHost.rebase("/elsewhere/a", canonicalRoot: "/private/var/app", requestedRoot: "/var/app")
                == "/elsewhere/a")
    }

    @Test func givenDaemonErrorCodesShouldMapToHostErrors() {
        #expect(HostError.from(code: "path_not_allowed", message: "no") == .pathNotAllowed("no"))
        #expect(HostError.from(code: "conflict", message: "changed") == .conflict("changed"))
        #expect(HostError.from(code: "not_found", message: "gone") == .notFound("gone"))
        #expect(HostError.from(code: "io_error", message: "x") == .daemon(code: "io_error", message: "x"))
    }
}

// MARK: - Remote layouts in the registry

@MainActor
struct RemoteRegistryTests {
    private let projectId = ProjectKey(machineId: UUID(), root: "/srv/app").id

    private func agent(_ id: String) -> AgentModel {
        AgentModel(id: id, name: id, agentType: "claude", status: .running, prompt: "", startedAt: "")
    }

    @Test func givenRemoteProjectShouldShowAgentsButNotSaveBeforeItsLayoutLoads() {
        let registry = WorkspaceRegistry(loadWorkspaces: { _ in
            Issue.record("a remote project must not load its layout from the local disk")
            return []
        })
        registry.reconcile(projectRoot: projectId, rootAgents: [agent("ag-a")], worktrees: [])
        // Agents show in a default layout while the machine's layout is still loading…
        #expect(registry.workspaces(forProject: projectId).map(\.id) == ["ws-ag-a"])

        // …but nothing is saved until it has been read (no saver yet).
        var saved: [[Workspace]] = []
        registry.saveAll()
        #expect(saved.isEmpty)
        registry.registerRemoteProject(projectId, restored: []) { workspaces, _ in saved.append(workspaces) }
        #expect(registry.workspaces(forProject: projectId).map(\.id) == ["ws-ag-a"])

        registry.saveAll()
        #expect(saved.last?.map(\.id) == ["ws-ag-a"])

        registry.forgetProject(projectId)
        #expect(registry.workspaces(forProject: projectId).isEmpty)
    }
}

// MARK: - Remote layout persistence

/// A scriptable host: reads and writes answer from queues of results, writes are recorded.
private final class FakeHost: ProjectHost, @unchecked Sendable {
    private let lock = NSLock()
    private var reads: [Result<HostFileData, HostError>]
    private var writeResults: [Result<HostFileStat, HostError>]
    private(set) var writes: [(data: Data, expectedModifiedMs: Int64?)] = []

    init(reads: [Result<HostFileData, HostError>], writes: [Result<HostFileStat, HostError>] = []) {
        self.reads = reads
        self.writeResults = writes
    }

    var isLocal: Bool { false }
    var identity: String { "machine:fake" }

    var writeCount: Int {
        lock.lock()
        defer { lock.unlock() }
        return writes.count
    }

    func read(path: String, maxBytes: Int) throws -> HostFileData {
        lock.lock()
        defer { lock.unlock() }
        let next = reads.count > 1 ? reads.removeFirst() : reads.first ?? .failure(.notFound(path))
        return try next.get()
    }

    func write(path: String, data: Data, expectedModifiedMs: Int64?, createDirectories: Bool) throws -> HostFileStat {
        lock.lock()
        defer { lock.unlock() }
        writes.append((data, expectedModifiedMs))
        let next =
            writeResults.isEmpty
            ? .success(HostFileStat(exists: true, isDirectory: false, size: 1, modifiedMs: 99))
            : writeResults.removeFirst()
        return try next.get()
    }

    func list(directory: String) throws -> [HostFileEntry] { [] }
    func listRecursive(root: String, limit: Int) throws -> (entries: [HostFileEntry], truncated: Bool) { ([], false) }
    func stat(path: String) throws -> HostFileStat { .missing }
    func delete(path: String) throws {}
    func run(_ command: HostCommand) -> CommandResult { CommandResult(stdout: "", stderr: "", exitCode: 0) }
    func makeWatcher(onChange: @escaping @Sendable () -> Void) -> any HostDirectoryWatcher {
        DaemonDirectoryWatcher(client: DaemonClient(), onChange: onChange)
    }
}

private func file(_ text: String, modifiedMs: Int64 = 10, truncated: Bool = false) -> Result<HostFileData, HostError> {
    .success(
        HostFileData(data: Data(text.utf8), size: UInt64(text.utf8.count), modifiedMs: modifiedMs, truncated: truncated)
    )
}

private let emptyLayoutDocument = #"{"version":3,"workspaces":[]}"#

struct RemoteLayoutPersistenceTests {
    @Test func givenNoLayoutFileShouldLoadAnEmptyLayout() throws {
        let host = FakeHost(reads: [.failure(.notFound("gone"))])
        let layout = try WorkspacePersistence.loadRemote(
            projectRoot: "/srv/app", projectId: "machine:x:/srv/app", host: host)
        #expect(layout.workspaces.isEmpty)
        #expect(layout.modifiedMs == nil)
    }

    @Test func givenFailedReadShouldThrowRatherThanLoadAnEmptyLayout() {
        let host = FakeHost(reads: [.failure(.daemon(code: "io_error", message: "connection reset"))])
        #expect(throws: HostError.self) {
            try WorkspacePersistence.loadRemote(projectRoot: "/srv/app", projectId: "p", host: host)
        }
        let large = FakeHost(reads: [file("{", truncated: true)])
        #expect(throws: RemoteLayoutError.self) {
            try WorkspacePersistence.loadRemote(projectRoot: "/srv/app", projectId: "p", host: large)
        }
    }

    @Test func givenUnreadableDocumentSaveShouldNotWrite() {
        let host = FakeHost(reads: [.failure(.daemon(code: "io_error", message: "x"))])
        let outcome = WorkspacePersistence.saveRemote(
            Data("{}".utf8), projectRoot: "/srv/app", host: host, expectedModifiedMs: 10)
        #expect(outcome == .skipped)
        #expect(host.writeCount == 0)
    }

    @Test func givenNewerDocumentSaveShouldNotDowngradeIt() {
        let host = FakeHost(reads: [file(#"{"version":99,"workspaces":[]}"#)])
        WorkspacePersistence.saveRemote(Data("{}".utf8), projectRoot: "/srv/app", host: host, expectedModifiedMs: nil)
        #expect(host.writeCount == 0)
    }

    @Test func givenUnchangedDocumentSaveShouldWriteConditionally() {
        let host = FakeHost(
            reads: [file(emptyLayoutDocument, modifiedMs: 5)],
            writes: [.success(HostFileStat(exists: true, isDirectory: false, size: 1, modifiedMs: 30))])
        let outcome = WorkspacePersistence.saveRemote(
            Data("{}".utf8), projectRoot: "/srv/app", host: host, expectedModifiedMs: 5)
        #expect(host.writes.map(\.expectedModifiedMs) == [5])
        #expect(outcome == .written(modifiedMs: 30))
    }

    @Test func givenDocumentChangedElsewhereSaveShouldAdoptItNotOverwrite() {
        // Another Mac wrote the layout after this app last read it (5 → 20).
        let theirs = #"{"version":3,"workspaces":[]}"#
        let host = FakeHost(reads: [file(theirs, modifiedMs: 20)])
        let outcome = WorkspacePersistence.saveRemote(
            Data("{}".utf8), projectRoot: "/srv/app", host: host, expectedModifiedMs: 5)
        #expect(outcome == .conflict(Data(theirs.utf8), modifiedMs: 20))
        #expect(host.writeCount == 0)
    }

    @Test func givenRaceBetweenReadAndWriteSaveShouldAdoptTheNewDocument() {
        let host = FakeHost(
            reads: [file(emptyLayoutDocument, modifiedMs: 5), file(emptyLayoutDocument, modifiedMs: 20)],
            writes: [.failure(.conflict("changed"))])
        let outcome = WorkspacePersistence.saveRemote(
            Data("{}".utf8), projectRoot: "/srv/app", host: host, expectedModifiedMs: 5)
        #expect(host.writeCount == 1)
        #expect(outcome == .conflict(Data(emptyLayoutDocument.utf8), modifiedMs: 20))
    }

    @Test func givenSaverShouldWriteAgainstTheLastKnownModificationTimeAndFlush() {
        let recorded = LockedValues()
        let saver = RemoteLayoutSaver { _, expected in
            recorded.append(expected)
            return .written(modifiedMs: (expected ?? 0) + 1)
        }
        saver.loaded(modifiedMs: 41)
        saver.enqueue(Data())
        saver.enqueue(Data())
        #expect(saver.flush(until: .now() + 2))
        #expect(recorded.values == [41, 42])
    }

    @Test func givenConflictQueuedWritesShouldNotRevertTheMachinesDocument() {
        let recorded = LockedValues()
        let adopted = LockedValues()
        let saver = RemoteLayoutSaver { _, expected in
            recorded.append(expected)
            return expected == 41 ? .conflict(Data("theirs".utf8), modifiedMs: 77) : .written(modifiedMs: 78)
        }
        saver.onConflict { document, _ in adopted.append(Int64(document.count)) }
        saver.loaded(modifiedMs: 41)
        // Two writes queued behind each other: the second must not write over the
        // machine's document before it has been adopted.
        saver.enqueue(Data())
        saver.enqueue(Data())
        #expect(saver.flush(until: .now() + 2))
        #expect(recorded.values == [41, 41])
        #expect(adopted.values == [6, 6])
        // Once adopted, the next write is conditional on the machine's version.
        saver.loaded(modifiedMs: 77)
        saver.enqueue(Data())
        #expect(saver.flush(until: .now() + 2))
        #expect(recorded.values == [41, 41, 77])
    }
}

private final class LockedValues: @unchecked Sendable {
    private let lock = NSLock()
    private var stored: [Int64?] = []
    func append(_ value: Int64?) { lock.lock(); stored.append(value); lock.unlock() }
    var values: [Int64?] { lock.lock(); defer { lock.unlock() }; return stored }
}

@MainActor
struct RemoteLayoutRestoreTests {
    private let projectId = ProjectKey(machineId: UUID(), root: "/srv/app").id

    @Test func givenFailedReadShouldSurfaceAnErrorAndNeverSave() async {
        let registry = WorkspaceRegistry(loadWorkspaces: { _ in [] })
        let host = FakeHost(reads: [.failure(.daemon(code: "io_error", message: "reset"))])
        let saver = RemoteLayoutSaver(projectRoot: "/srv/app", host: host)
        let task = Task {
            await ProjectState.restoreRemoteLayout(
                projectRoot: "/srv/app", projectId: projectId, host: host, registry: registry, saver: saver,
                retryDelay: { _ in .milliseconds(20) })
        }
        for _ in 0..<100 where registry.layoutErrorsByProject[projectId] == nil {
            try? await Task.sleep(for: .milliseconds(10))
        }
        #expect(registry.layoutErrorsByProject[projectId]?.contains("reset") == true)

        registry.reconcile(
            projectRoot: projectId,
            rootAgents: [
                AgentModel(id: "ag-a", name: "a", agentType: "claude", status: .running, prompt: "", startedAt: "")
            ],
            worktrees: [])
        // The agents show while the layout is unreadable; nothing is saved meanwhile.
        #expect(registry.workspaces(forProject: projectId).map(\.id) == ["ws-ag-a"])
        registry.saveAll()
        registry.forgetProject(projectId)
        #expect(saver.flush(until: .now() + 1))
        #expect(host.writeCount == 0)
        #expect(registry.workspaces(forProject: projectId).isEmpty)
        task.cancel()
        await task.value
    }

    @Test func givenReadRecoversShouldRegisterTheLayout() async {
        let registry = WorkspaceRegistry(loadWorkspaces: { _ in [] })
        let host = FakeHost(reads: [.failure(.daemon(code: "io_error", message: "reset")), .failure(.notFound("none"))])
        let saver = RemoteLayoutSaver(projectRoot: "/srv/app", host: host)
        await ProjectState.restoreRemoteLayout(
            projectRoot: "/srv/app", projectId: projectId, host: host, registry: registry, saver: saver,
            retryDelay: { _ in .milliseconds(10) })
        #expect(registry.layoutErrorsByProject[projectId] == nil)
        registry.reconcile(
            projectRoot: projectId,
            rootAgents: [
                AgentModel(id: "ag-a", name: "a", agentType: "claude", status: .running, prompt: "", startedAt: "")
            ],
            worktrees: [])
        #expect(registry.workspaces(forProject: projectId).map(\.id) == ["ws-ag-a"])
        registry.saveAll()
        #expect(saver.flush(until: .now() + 1))
        #expect(host.writeCount == 1)
        registry.forgetProject(projectId)
    }
}

// MARK: - Remote git and watching

struct RemoteExecTests {
    private func exec(truncated: Bool) -> DaemonResponse {
        .gitExecResult(exitCode: 0, stdout: Data("partial".utf8), stderr: Data(), truncated: truncated)
    }

    @Test func givenTruncatedOutputWithoutLimitShouldFail() {
        let command = HostCommand(program: "git", executablePath: "/usr/bin/git", args: ["status"], cwd: "/srv/app")
        let result = DaemonProjectHost.commandResult(exec(truncated: true), for: command)
        #expect(!result.success)
        #expect(result.stdout.isEmpty)
        #expect(DaemonProjectHost.commandResult(exec(truncated: false), for: command).stdout == "partial")
    }

    @Test func givenTruncatedOutputWithLimitShouldReportItExceeded() {
        let command = HostCommand(
            program: "git", executablePath: "/usr/bin/git", args: ["diff"], cwd: "/srv/app", outputLimit: 1_000)
        #expect(DaemonProjectHost.commandResult(exec(truncated: true), for: command).outputExceededLimit)
    }

    @Test func givenRepeatedFailuresBackoffShouldCapAtFifteenSecondsAndReset() {
        var backoff = SubscriptionBackoff()
        let delays = (0..<8).map { _ in backoff.next() }
        #expect(delays.prefix(5) == [.seconds(1), .seconds(2), .seconds(4), .seconds(8), .seconds(15)])
        #expect(delays.last == .seconds(15))
        backoff.reset()
        #expect(backoff.next() == .seconds(1))
    }
}

@MainActor
struct WorkspaceFilesIdentityTests {
    @Test func givenSamePathOnAnotherHostShouldBeADifferentFilesState() {
        let remote = FakeHost(reads: [])
        let local = WorkspaceFilesState.identity(rootPath: "/srv/app", host: LocalProjectHost.shared)
        #expect(local != WorkspaceFilesState.identity(rootPath: "/srv/app", host: remote))
        #expect(local == WorkspaceFilesState(rootPath: "/srv/app").identity)
    }
}

// MARK: - Remote layout conflicts and quit

@MainActor
struct RemoteLayoutConflictTests {
    private let projectId = ProjectKey(machineId: UUID(), root: "/srv/app").id
    private let agent = AgentModel(
        id: "ag-a", name: "a", agentType: "claude", status: .running, prompt: "", startedAt: "")

    @Test func givenUnchangedRemoteLayoutQuitShouldNotRewriteIt() async {
        let registry = WorkspaceRegistry(loadWorkspaces: { _ in [] })
        var saves = 0
        registry.registerRemoteProject(projectId, restored: []) { _, _ in saves += 1 }
        registry.reconcile(projectRoot: projectId, rootAgents: [agent], worktrees: [])
        registry.saveAll()
        #expect(saves == 1)
        // Nothing changed since: a second quit-time flush must not rewrite the machine's
        // layout (another Mac may have changed it).
        registry.saveAll()
        #expect(saves == 1)
        registry.forgetProject(projectId)
    }

    @Test func givenConflictShouldAdoptTheMachinesLayout() {
        let registry = WorkspaceRegistry(loadWorkspaces: { _ in [] })
        registry.registerRemoteProject(projectId, restored: []) { _, _ in }
        registry.reconcile(projectRoot: projectId, rootAgents: [agent], worktrees: [])
        // The machine's copy differs (another Mac advanced its surface counter).
        let theirs = registry.workspaces(forProject: projectId).map { workspace -> Workspace in
            var changed = workspace
            changed.nextSurfaceId += 50
            return changed
        }
        registry.adoptRemoteLayout(projectId, workspaces: theirs)
        #expect(registry.workspaces(forProject: projectId) == theirs)
        registry.forgetProject(projectId)
    }
}

// MARK: - Editors on remote machines

@MainActor
struct RemoteEditorTests {
    private let machine = UUID()

    private func editor(host: any ProjectHost, path: String, dirty: Bool) -> EditorState {
        let editor = EditorState()
        editor.host = host
        editor.currentFile = EditorTab(
            id: path, name: (path as NSString).lastPathComponent, content: "x", isDirty: dirty, lastModified: nil,
            isBinary: false, language: .plaintext)
        return editor
    }

    @Test func givenDirtyRemoteEditorShouldBeReportedForItsProjectOnly() {
        let host = DaemonProjectHost(client: DaemonClient(), machineId: machine)
        let dirty = editor(host: host, path: "/srv/app/.pu/worktrees/wt-1/main.rs", dirty: true)
        let clean = editor(host: host, path: "/srv/app/lib.rs", dirty: false)
        let local = editor(host: LocalProjectHost.shared, path: "/srv/app/local.rs", dirty: true)
        let elsewhere = editor(host: host, path: "/srv/other/x.rs", dirty: true)
        #expect(OpenEditors.unsavedFileNames(hostIdentity: host.identity, root: "/srv/app") == ["main.rs"])
        withExtendedLifetime([dirty, clean, local, elsewhere]) {}
    }

    @Test func givenLargeRemoteContentShouldOpenReadOnly() {
        let remote = DaemonProjectHost(client: DaemonClient(), machineId: machine)
        let large = String(repeating: "a", count: HostError.maxRemoteSaveBytes + 1)
        #expect(EditorState.isTooLargeToSave(large, host: remote))
        #expect(!EditorState.isTooLargeToSave("small", host: remote))
        #expect(!EditorState.isTooLargeToSave(large, host: LocalProjectHost.shared))
    }

    @Test func givenReadOnlyFileEditsShouldBeIgnored() {
        let editor = EditorState()
        editor.currentFile = EditorTab(
            id: "/srv/app/big.log", name: "big.log", content: "original", isDirty: false, lastModified: nil,
            isBinary: false, language: .plaintext, isReadOnly: true)
        editor.updateContent(content: "changed")
        #expect(editor.currentFile?.content == "original")
        #expect(editor.currentFile?.isDirty == false)
        #expect(editor.readOnlyReason != nil)
    }

    @Test func givenUnavailableHostEveryOperationShouldFail() {
        let host = UnavailableProjectHost()
        #expect(!host.isLocal)
        #expect(throws: HostError.self) { try host.read(path: "/etc/hosts", maxBytes: 10) }
        #expect(throws: HostError.self) {
            try host.write(path: "/tmp/x", data: Data(), expectedModifiedMs: nil, createDirectories: false)
        }
        #expect(
            !host.run(HostCommand(program: "git", executablePath: "/usr/bin/git", args: ["status"], cwd: "/")).success)
    }
}

struct LocalCopyTests {
    @Test func givenLocalCopyShouldBeQuarantined() throws {
        let url = FileManager.default.temporaryDirectory.appendingPathComponent(
            "pp-quarantine-\(UUID().uuidString).command")
        try Data("echo hi".utf8).write(to: url)
        defer { try? FileManager.default.removeItem(at: url) }
        FileIOService.quarantine(url)
        #expect(getxattr(url.path, "com.apple.quarantine", nil, 0, 0, 0) > 0)
    }
}
