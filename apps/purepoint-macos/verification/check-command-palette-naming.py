#!/usr/bin/env python3
"""Test the actual AppKit palette in a temporary package; no app build phases or daemon."""
from pathlib import Path
import os
import shutil
import subprocess
import tempfile

app = Path(__file__).resolve().parents[1]
source = app / "purepoint-macos"
with tempfile.TemporaryDirectory(prefix="palette-naming-", dir="/tmp") as folder:
    package = Path(folder)
    sources = package / "Sources/PurePoint"
    tests = package / "Tests/PurePointTests"
    sources.mkdir(parents=True)
    tests.mkdir(parents=True)
    (package / "Package.swift").write_text('''// swift-tools-version: 6.0
import PackageDescription
let package = Package(name: "PaletteNamingChecks", platforms: [.macOS(.v15)], targets: [
    .target(name: "PurePoint", swiftSettings: [.unsafeFlags(["-default-isolation", "MainActor", "-enable-upcoming-feature", "NonisolatedNonsendingByDefault", "-enable-upcoming-feature", "InferIsolatedConformances", "-warnings-as-errors"])]),
    .testTarget(name: "PurePointTests", dependencies: ["PurePoint"])
], swiftLanguageModes: [.v5])
''')
    for name in [
        "Models/CommandPaletteItem.swift", "Models/AgentVariant.swift", "Models/AgentsHubModels.swift",
        "Models/EditorTab.swift", "State/SettingsState.swift", "State/CommandPaletteOpenCoordinator.swift",
        "Utilities/WorktreeNameNormalizer.swift", "Views/CommandPalette/CommandPalettePanel.swift",
        "Views/CommandPalette/CommandPaletteRowView.swift"
    ]:
        shutil.copyfile(source / name, sources / Path(name).name)
    # Use the production definition payloads without pulling in unrelated daemon services.
    protocol = (source / "Services/DaemonProtocol.swift").read_text()
    payloads = protocol[protocol.index("nonisolated struct TemplateInfo:"):protocol.index("// MARK: - Response payload helpers")]
    roster = protocol[protocol.index("nonisolated struct SwarmRosterEntryPayload:"):protocol.index("nonisolated enum ScheduleTriggerPayload:")]
    (sources / "DefinitionPayloads.swift").write_text("import Foundation\n" + roster + payloads)
    for name in ["CommandPaletteNamingTests.swift", "CommandPaletteOrderTests.swift", "CommandPaletteOpenCoordinatorTests.swift"]:
        shutil.copyfile(app / "purepoint-macosTests" / name, tests / name)
    environment = dict(os.environ, CFFIXED_USER_HOME=str(package / "home"))
    (package / "home").mkdir()
    subprocess.run(["xcrun", "swift", "test", "--package-path", str(package)], check=True, env=environment)
