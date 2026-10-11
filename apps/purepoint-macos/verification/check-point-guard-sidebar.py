#!/usr/bin/env python3
"""Render the real nested sidebar/grid layout without any app build phases."""
from pathlib import Path
import os
import subprocess
import tempfile

app = Path(__file__).resolve().parents[1]
source = app / "purepoint-macos"
with tempfile.TemporaryDirectory(prefix="point-guard-sidebar-checks-") as folder:
    executable = Path(folder) / "checks"
    subprocess.run([
        "xcrun", "swiftc", "-parse-as-library", "-strict-concurrency=complete", "-warnings-as-errors", "-swift-version", "5",
        str(source / "Models/PiChatDomain.swift"),
        str(source / "Services/PiPairingSecret.swift"),
        str(source / "State/PiChatModel.swift"),
        str(source / "Services/PiAttachmentImport.swift"),
        str(source / "Theme/PurePointTheme.swift"),
        str(source / "Views/DaemonErrorBanner.swift"),
        str(source / "Views/Chat/MarkdownTextView.swift"),
        str(source / "Views/Chat/CodeBlockView.swift"),
        str(source / "Views/Chat/PiPointGuardChatView.swift"),
        str(source / "Views/Chat/PointGuardView.swift"),
        str(source / "Views/Chat/PointGuardPaneControls.swift"),
        str(source / "Views/Chat/PointGuardConversationLayout.swift"),
        str(source / "Views/PaneGrid/DraggableSplit.swift"),
        str(app / "verification/PointGuardSidebarChecks.swift"),
        "-o", str(executable),
    ], check=True)
    subprocess.run([str(executable)], env=dict(os.environ, CFFIXED_USER_HOME=folder), check=True, timeout=30)
