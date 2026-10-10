#!/usr/bin/env python3
"""Compile actual mobile trust/model sources and exercise an isolated real TLS fixture."""
import os
import plistlib
from pathlib import Path
import subprocess
import tempfile
root = Path(__file__).resolve().parent.parent
ats = plistlib.loads((root / 'PurePoint/Info.plist').read_bytes())['NSAppTransportSecurity']
assert ats.get('NSAllowsArbitraryLoads') is True, 'QR-pinned TLS at user-selected IPs requires custom trust evaluation'
assert not any(key in ats for key in ('NSAllowsLocalNetworking', 'NSAllowsArbitraryLoadsForMedia', 'NSAllowsArbitraryLoadsInWebContent')), 'Fine-grained ATS keys override NSAllowsArbitraryLoads on iOS and break IP pairing'
with tempfile.TemporaryDirectory(prefix='pg-mobile-trust-', dir='/tmp') as directory:
    temp = Path(directory)
    recovery = (root / 'verification/RecoveryChecks.swift').read_text().split('// Run verification/')[0]
    (temp / 'TrustModel.swift').write_text((root / 'PurePoint/ChatModel.swift').read_text() + '\n' + recovery + '\n' + (root / 'verification/TrustChecks.swift').read_text())
    subprocess.run(['swiftc', '-parse-as-library', '-strict-concurrency=complete', '-warnings-as-errors', str(root / 'PurePoint/ChatDomain.swift'), str(root / 'PurePoint/PairingSecret.swift'), str(temp / 'TrustModel.swift'), '-o', str(temp / 'checks')], check=True)
    fixture = subprocess.Popen(['node', str(root / 'verification/trust-fixture.js'), directory], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)
    try:
        if fixture.stdout.readline().strip() != 'ready':
            raise RuntimeError('Fixture did not become ready')
        env = os.environ.copy(); env['CFFIXED_USER_HOME'] = directory
        subprocess.run([str(temp / 'checks'), directory], env=env, check=True, timeout=30)
        fixture.stdin.write('count\n'); fixture.stdin.flush()
        counts = fixture.stdout.readline().strip(); print(counts)
        if not counts.endswith(';prompts=0'): raise RuntimeError('Reconnect replayed a prompt')
    finally:
        fixture.stdin.write('stop\n'); fixture.stdin.flush(); fixture.wait(timeout=10)
