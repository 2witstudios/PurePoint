#!/usr/bin/env python3
"""Compile an isolated iOS simulator chat fixture without Xcode app build phases.

Uses the real phone view/model/domain sources. Installs only a separate verification
bundle on an already-booted simulator. No bridge, production app, daemon or Cargo
binary is changed. Requires local Xcode and an arm64 iOS simulator.
"""
import os
from pathlib import Path
import plistlib
import subprocess

root = Path(__file__).resolve().parent
mobile = root.parent.parent / 'purepoint-mobile'
output = Path('/tmp/pointguard-phone-preview')
app = output / 'PointGuardMarkdownPreview.app'
app.mkdir(parents=True, exist_ok=True)
env = dict(os.environ, DEVELOPER_DIR='/Applications/Xcode.app/Contents/Developer')

def run(args):
    return subprocess.run(args, check=True, env=env)

sdk = subprocess.check_output(['xcrun', '--sdk', 'iphonesimulator', '--show-sdk-path'], env=env, text=True).strip()
sources = [str(p) for p in (mobile / 'PurePoint').glob('*.swift') if p.name != 'PurePointApp.swift']
run(['xcrun', '--sdk', 'iphonesimulator', 'swiftc', '-parse-as-library', '-swift-version', '5', '-target', 'arm64-apple-ios17.0-simulator', '-sdk', sdk, *sources, str(root / 'ChatMarkdown.swift'), str(root / 'ChatMarkdownView.swift'), str(root / 'ChatBubble.swift'), str(root / 'PhonePreview.swift'), '-o', str(app / 'PointGuardMarkdownPreview')])
with (app / 'Info.plist').open('wb') as stream:
    plistlib.dump({
        'CFBundleExecutable': 'PointGuardMarkdownPreview',
        'CFBundleIdentifier': 'dev.purepoint.MarkdownVerification',
        'CFBundleName': 'Markdown preview',
        'CFBundlePackageType': 'APPL',
        'CFBundleVersion': '1',
        'CFBundleShortVersionString': '1.0',
        'CFBundleSupportedPlatforms': ['iPhoneSimulator'],
        'UIDeviceFamily': [1, 2],
        'MinimumOSVersion': '17.0',
        'UIApplicationSceneManifest': {'UIApplicationSupportsMultipleScenes': False},
        'UILaunchScreen': {},
    }, stream)
run(['xcrun', 'actool', str(mobile / 'PurePoint/Assets.xcassets'), '--compile', str(app), '--platform', 'iphonesimulator', '--minimum-deployment-target', '17.0', '--target-device', 'iphone', '--target-device', 'ipad', '--accent-color', 'AccentColor', '--output-partial-info-plist', str(output / 'asset-info.plist')])
with (output / 'asset-info.plist').open('rb') as stream:
    asset_info = plistlib.load(stream)
with (app / 'Info.plist').open('rb') as stream:
    info = plistlib.load(stream)
info.update(asset_info)
with (app / 'Info.plist').open('wb') as stream:
    plistlib.dump(info, stream)
run(['codesign', '--force', '--sign', '-', str(app)])
run(['xcrun', 'simctl', 'install', 'booted', str(app)])
run(['xcrun', 'simctl', 'launch', '--terminate-running-process', 'booted', 'dev.purepoint.MarkdownVerification'])
