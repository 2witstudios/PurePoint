#!/usr/bin/env python3
"""Render the actual desktop chat in an isolated native host, with app chrome stubs.

No Xcode app build phase, Cargo build, bridge connection or installed binary is used.
"""
import os
from pathlib import Path
import subprocess
import tempfile

root = Path(__file__).resolve().parent
mac = root.parent.parent / 'purepoint-macos/purepoint-macos'
files = [root / 'ChatMarkdown.swift', root / 'ChatMarkdownView.swift', root / 'ChatBubble.swift', root / 'DesktopChatPreview.swift',
         mac / 'Views/Chat/PiPointGuardChatView.swift', mac / 'Models/PiChatDomain.swift',
         mac / 'State/PiChatModel.swift', mac / 'Services/PiPairingSecret.swift',
         mac / 'Services/PiAttachmentImport.swift', mac / 'Theme/PurePointTheme.swift']
env = dict(os.environ, DEVELOPER_DIR='/Applications/Xcode.app/Contents/Developer')
sdk = subprocess.check_output(['xcrun', '--sdk', 'macosx', '--show-sdk-path'], env=env, text=True).strip()
platform_developer = Path(sdk).parents[1]
plugin = str(platform_developer / 'usr/lib/swift/host/plugins') + '#' + str(platform_developer / 'usr/bin/swift-plugin-server')
with tempfile.TemporaryDirectory(prefix='pointguard-desktop-preview-') as temporary:
    binary = Path(temporary) / 'preview'
    subprocess.run(['xcrun', 'swiftc', '-parse-as-library', '-swift-version', '5', '-sdk', sdk, '-external-plugin-path', plugin, *map(str, files), '-o', str(binary)], env=env, check=True)
    subprocess.run([str(binary)], env=dict(env, CFFIXED_USER_HOME=temporary), check=True, timeout=30)
