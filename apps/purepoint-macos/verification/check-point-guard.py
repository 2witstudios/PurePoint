#!/usr/bin/env python3
"""Safe standalone Swift/real bridge integration; no Xcode builds or native Pi data."""
import json
import os
from pathlib import Path
import secrets
import subprocess
import tempfile

root = Path(__file__).resolve().parents[1]
mobile = root.parent / 'purepoint-mobile'
source = root / 'purepoint-macos'
with tempfile.TemporaryDirectory(prefix='pointguard-checks-', dir='/tmp') as folder:
    temporary = Path(folder)
    combined = temporary / 'ModelChecks.swift'
    combined.write_text((source / 'State/PiChatModel.swift').read_text() + '\n' + (mobile / 'PurePoint/ChatModel.swift').read_text() + '\n' + (root / 'verification/PiChatChecks.swift').read_text())
    subprocess.run(['swiftc', '-parse-as-library', '-strict-concurrency=complete', '-warnings-as-errors', str(source / 'Models/PiChatDomain.swift'), str(source / 'Services/PiPairingSecret.swift'), str(combined), str(mobile / 'PurePoint/ChatDomain.swift'), str(mobile / 'PurePoint/PairingSecret.swift'), str(mobile / 'PurePoint/AgentControlDomain.swift'), str(mobile / 'PurePoint/AgentControlModel.swift'), '-o', str(temporary / 'checks')], check=True)
    env = dict(os.environ, CFFIXED_USER_HOME=folder, POINTGUARD_FIXTURE_TOKEN=secrets.token_hex(32))
    bridge = subprocess.Popen(['node', str(root / 'verification/pi-fixture.mjs'), str(mobile)], env=env, stdout=subprocess.PIPE, text=True)
    try:
        ready = json.loads(bridge.stdout.readline().strip())
        port = str(ready['port'])
        if not port.isdigit():
            raise RuntimeError('Fixture did not start; run npm ci --ignore-scripts in apps/purepoint-mobile')
        env['POINTGUARD_FIXTURE_PORT'] = port
        env['POINTGUARD_FIXTURE_CLIENT_ID'] = ready['desktopClientId']
        subprocess.run([str(temporary / 'checks')], env=env, check=True, timeout=60)
    finally:
        bridge.terminate()
        try:
            bridge.wait(timeout=5)
        except subprocess.TimeoutExpired:
            bridge.kill()
            bridge.wait()

# Desktop and mobile now have distinct persistence schemas. Keep the existing
# single-host desktop recovery assertions against its own actual model.
with tempfile.TemporaryDirectory(prefix='pointguard-checks-', dir='/tmp') as folder:
    temporary = Path(folder)
    checks = (root / 'verification/PiRecoveryChecks.swift').read_text()
    combined = temporary / 'RecoveryModel.swift'
    combined.write_text((source / 'State/PiChatModel.swift').read_text() + '\n' + checks)
    subprocess.run(['swiftc', '-parse-as-library', '-strict-concurrency=complete', '-warnings-as-errors', str(source / 'Models/PiChatDomain.swift'), str(source / 'Services/PiPairingSecret.swift'), str(combined), '-o', str(temporary / 'checks')], check=True)
    subprocess.run([str(temporary / 'checks')], env=dict(os.environ, CFFIXED_USER_HOME=folder), check=True, timeout=60)
