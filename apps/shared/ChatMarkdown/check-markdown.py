#!/usr/bin/env python3
"""Run isolated parser tests without any app or daemon build phases."""
from pathlib import Path
import tempfile, subprocess, os
root = Path(__file__).resolve().parent
with tempfile.TemporaryDirectory(prefix='pointguard-markdown-') as folder:
    p = Path(folder)
    (p/'Sources/ChatMarkdown').mkdir(parents=True)
    (p/'Tests/ChatMarkdownTests').mkdir(parents=True)
    (p/'Package.swift').write_text('// swift-tools-version: 6.0\nimport PackageDescription\nlet package = Package(name: "ChatMarkdown", platforms: [.macOS(.v14)], targets: [.target(name: "ChatMarkdown"), .testTarget(name: "ChatMarkdownTests", dependencies: ["ChatMarkdown"])])')
    source=root/'ChatMarkdown.swift'
    (p/'Sources/ChatMarkdown/ChatMarkdown.swift').write_text(source.read_text())
    (p/'Tests/ChatMarkdownTests/ChatMarkdownTests.swift').write_text('@testable import ChatMarkdown\n'+(root/'ChatMarkdownTests.swift').read_text())
    env=dict(os.environ, DEVELOPER_DIR='/Applications/Xcode.app/Contents/Developer')
    raise SystemExit(subprocess.call(['swift','test','--package-path',folder], env=env))
