#!/usr/bin/env python3
"""Render the real nested sidebar/grid layout without any app build phases."""
from pathlib import Path
import os
import subprocess
import tempfile

app = Path(__file__).resolve().parents[1]
source = app / "purepoint-macos"
markdown = app.parent / "shared/ChatMarkdown"
with tempfile.TemporaryDirectory(prefix="point-guard-sidebar-checks-") as folder:
    executable = Path(folder) / "checks"
    # Compile the production root-safety method against the fixture AppState.
    # Keeping its implementation in the source list prevents an always-safe stub
    # from hiding changes to PointGuardView's project-root guard.
    state_source = (source / "State/AppState.swift").read_text()
    signature = "    func isTooBroadProjectRoot(_ path: String) -> Bool {"
    method_start = state_source.index(signature)
    method, separator, _ = state_source[method_start:].partition("\n    }\n")
    if not separator:
        raise RuntimeError("Cannot locate production AppState root-safety method")
    root_safety = Path(folder) / "AppStateRootSafety.swift"
    root_safety.write_text("import Foundation\nextension AppState {\n" + method + separator + "}\n")
    subprocess.run([
        "xcrun", "swiftc", "-parse-as-library", "-strict-concurrency=complete", "-warnings-as-errors", "-swift-version", "5",
        str(source / "Models/PiChatDomain.swift"),
        str(source / "Services/PiPairingSecret.swift"),
        str(source / "State/PiChatModel.swift"),
        str(source / "Services/PiAttachmentImport.swift"),
        str(source / "Theme/PurePointTheme.swift"),
        str(source / "Theme/Theme.swift"),
        str(source / "Views/DaemonErrorBanner.swift"),
        str(markdown / "ChatMarkdown.swift"),
        str(markdown / "ChatMarkdownView.swift"),
        str(markdown / "ChatBubble.swift"),
        str(source / "Views/Chat/MarkdownTextView.swift"),
        str(source / "Views/Chat/CodeBlockView.swift"),
        str(source / "Views/Chat/PiPointGuardChatView.swift"),
        str(source / "Views/Chat/PointGuardCourtView.swift"),
        str(source / "Models/PointGuardPlays.swift"),
        str(source / "Views/Chat/PiTranscriptScrollCoordinator.swift"),
        str(source / "Views/Chat/PointGuardView.swift"),
        str(source / "Views/Chat/PointGuardPaneControls.swift"),
        str(source / "Views/Chat/PointGuardConversationLayout.swift"),
        str(source / "Views/PaneGrid/DraggableSplit.swift"),
        str(source / "Views/PresentedControlDefaults.swift"),
        str(app / "verification/PointGuardSidebarChecks.swift"),
        str(root_safety),
        "-o", str(executable),
    ], check=True)
    subprocess.run([str(executable)], env=dict(os.environ, CFFIXED_USER_HOME=folder), check=True, timeout=60)
