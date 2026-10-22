#!/usr/bin/env python3
"""Run pane/model tests in an isolated package, without the macOS app build phases."""
from pathlib import Path
import os
import shutil
import subprocess
import tempfile

app = Path(__file__).resolve().parents[1]
source = app / "purepoint-macos"

with tempfile.TemporaryDirectory(prefix="point-guard-pane-checks-") as directory:
    package = Path(directory)
    sources = package / "Sources/PurePoint"
    tests = package / "Tests/PurePointTests"
    sources.mkdir(parents=True)
    tests.mkdir(parents=True)
    (package / "Package.swift").write_text('''// swift-tools-version: 6.0
import PackageDescription
let package = Package(
    name: "PointGuardPaneChecks", platforms: [.macOS(.v15)],
    targets: [
        .target(name: "PurePoint", swiftSettings: [.unsafeFlags(["-strict-concurrency=complete", "-warnings-as-errors"])]),
        .testTarget(name: "PurePointTests", dependencies: ["PurePoint"])
    ],
    swiftLanguageModes: [.v5])
''')
    for name in [
        "Models/Workspace.swift", "Models/WorkspaceModel.swift", "Models/AgentStatus.swift",
        "Models/ManifestModel.swift", "Models/ChannelModel.swift", "Views/PaneGrid/PaneSplitNode.swift",
        "State/AppState.swift", "Services/WorkspaceService.swift", "Services/DaemonProtocol.swift",
        "Views/PaneGrid/WorkspacePersistence.swift", "State/WorkspaceRegistry.swift",
        "Views/Chat/PointGuardView.swift", "Theme/Theme.swift",
        "Models/ProjectKey.swift", "Models/Machine.swift",
        "Models/AgentSignalSnapshot.swift", "Models/AgentSignalRouting.swift", "Models/AgentSignalPolicy.swift"
    ]:
        shutil.copyfile(source / name, sources / Path(name).name)

    shutil.copyfile(app / "verification/PointGuardRoutingStubs.swift", sources / "RoutingStubs.swift")
    project = (source / "State/ProjectState.swift").read_text()
    spawn = project[project.index("    func spawnAgentForSurface("):
                    project.index("    func createWorktree(")]
    (sources / "SurfaceSpawn.swift").write_text("import Foundation\n@MainActor extension ProjectState {\n" + spawn + "}\n")
    connection = (source / "Services/DaemonConnection.swift").read_text()
    hex_code = connection[connection.index("nonisolated private let hexDigits"):
                          connection.index("// MARK: - Errors")]
    (sources / "DataHex.swift").write_text("import Foundation\n" + hex_code)
    shutil.copyfile(app / "verification/PointGuardSpawnRoutingChecks.swift", tests / "PointGuardSpawnRoutingChecks.swift")
    # File editor/watchers are outside these tests; registry/pane/persistence code is real.
    (sources / "FileTabStoreStub.swift").write_text('''import Foundation
@MainActor final class FileTabStore {
    struct Key: Hashable { let workspaceId: String; let surfaceId: Int }
    private var sessions: [Key: FileTabSession] = [:]
    func retain(only keys: Set<Key>) { sessions = sessions.filter { keys.contains($0.key) } }
    func existingSession(workspaceId: String, surfaceId: Int) -> FileTabSession? {
        sessions[Key(workspaceId: workspaceId, surfaceId: surfaceId)]
    }
    func session(workspaceId: String, surfaceId: Int, initialPath: String?) -> FileTabSession {
        let key = Key(workspaceId: workspaceId, surfaceId: surfaceId)
        if let existing = sessions[key] { return existing }
        let session = FileTabSession()
        sessions[key] = session
        return session
    }
}
@MainActor final class FileTabSession {
    struct File { let name: String }
    struct Editor { var currentFile: File? }
    var editor = Editor()
    var hasUnsavedChanges: Bool { false }
}
''')
    for name in [
        "PointGuardWorkspaceTests.swift", "PointGuardRoutingTests.swift", "PaneTabStackTests.swift",
        "WorkspaceReconcilerTests.swift", "WorkspaceModelTests.swift", "WorkspaceRegistryTests.swift"
    ]:
        shutil.copyfile(app / "purepoint-macosTests" / name, tests / name)
    # Include the production persistence suite that also runs in the app's CI target.
    app_tests = (app / "purepoint-macosTests/purepoint_macosTests.swift").read_text()
    persistence_tests = app_tests[app_tests.index("struct GridLayoutPersistenceTests {"):]
    (tests / "GridLayoutPersistenceTests.swift").write_text(
        "import Foundation\nimport Testing\n@testable import PurePoint\n" + persistence_tests)
    environment = dict(os.environ, CFFIXED_USER_HOME=str(package / "home"))
    (package / "home").mkdir()
    subprocess.run(["xcrun", "swift", "test", "--package-path", str(package)], check=True, env=environment)
