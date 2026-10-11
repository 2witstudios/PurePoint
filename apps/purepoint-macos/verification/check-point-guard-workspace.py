#!/usr/bin/env python3
"""Run pane/model tests in an isolated package, without the macOS app build phases."""
from pathlib import Path
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
        "Models/ManifestModel.swift", "Views/PaneGrid/PaneSplitNode.swift",
        "Views/PaneGrid/WorkspacePersistence.swift", "State/WorkspaceRegistry.swift"
    ]:
        shutil.copyfile(source / name, sources / Path(name).name)

    protocol = (source / "Services/DaemonProtocol.swift").read_text()
    grid = protocol[protocol.index("nonisolated struct GridCommandPayload"):
                    protocol.index("nonisolated struct SwarmRosterEntryPayload")]
    (sources / "GridProtocol.swift").write_text("import Foundation\n" + grid)
    # File editor/watchers are outside these tests; registry/pane/persistence code is real.
    (sources / "FileTabStoreStub.swift").write_text('''import Foundation
@MainActor final class FileTabStore {
    struct Key: Hashable { let workspaceId: String; let surfaceId: Int }
    func retain(only keys: Set<Key>) {}
}
''')
    for name in [
        "PointGuardWorkspaceTests.swift", "PaneTabStackTests.swift",
        "WorkspaceReconcilerTests.swift", "WorkspaceModelTests.swift"
    ]:
        shutil.copyfile(app / "purepoint-macosTests" / name, tests / name)
    # Include the production persistence suite that also runs in the app's CI target.
    app_tests = (app / "purepoint-macosTests/purepoint_macosTests.swift").read_text()
    persistence_tests = app_tests[app_tests.index("struct GridLayoutPersistenceTests {"):]
    (tests / "GridLayoutPersistenceTests.swift").write_text(
        "import Foundation\nimport Testing\n@testable import PurePoint\n" + persistence_tests)
    subprocess.run(["xcrun", "swift", "test", "--package-path", str(package)], check=True)
