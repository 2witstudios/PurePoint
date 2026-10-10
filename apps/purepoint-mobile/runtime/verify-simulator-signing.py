#!/usr/bin/env python3
"""Read-only verification of Xcode's embedded simulator access identity.

Simulator iOS entitlements belong in __TEXT,__entitlements. Do not attach iOS
restricted entitlements to the macOS ad-hoc code signature: taskgated rejects it.
The actual Keychain XCTest remains the acceptance proof.
"""
import argparse
import os
from pathlib import Path
import plistlib
import re
import subprocess

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--app', required=True)
parser.add_argument('--diagnostics', required=True)
args = parser.parse_args()
app = Path(args.app).resolve()
root = Path(os.environ['RUNNER_TEMP']).resolve()
assert root in app.parents, 'Inspect only current CI temporary artifacts'
info = plistlib.loads((app / 'Info.plist').read_bytes())
assert 'iPhoneSimulator' in info.get('CFBundleSupportedPlatforms', []), 'Refuse non-simulator artifacts'
identifier = info['CFBundleIdentifier']
diagnostics = Path(args.diagnostics)
diagnostics.mkdir(parents=True, exist_ok=True)
result = subprocess.run(['/usr/bin/codesign', '--display', '--entitlements', ':-', str(app)], capture_output=True, check=True)
(diagnostics / 'codesign-stderr.txt').write_bytes(result.stderr)
current = plistlib.loads(result.stdout) if result.stdout.strip() else {}
(diagnostics / 'signature-entitlements.plist').write_bytes(plistlib.dumps(current))
assert not current.get('application-identifier') and not current.get('keychain-access-groups'), 'iOS access rights must be embedded, not added to the host macOS signature'
executable = app / info['CFBundleExecutable']
slices = subprocess.check_output(['/usr/bin/lipo', '-archs', str(executable)], text=True).split()
for architecture in slices:
    dump = subprocess.check_output(['/usr/bin/otool', '-arch', architecture, '-s', '__TEXT', '__entitlements', str(executable)], text=True)
    (diagnostics / f'embedded-{architecture}.txt').write_text(dump)
    words = []
    for line in dump.splitlines():
        fields = line.split()
        if fields and re.fullmatch(r'[0-9a-fA-F]{8,16}', fields[0]):
            words.extend(word for word in fields[1:] if re.fullmatch(r'(?:[0-9a-fA-F]{8}|[0-9a-fA-F]{2})', word))
    assert architecture in ['arm64', 'x86_64'], 'Unsupported simulator slice'
    data = b''.join(int(word, 16).to_bytes(len(word) // 2, 'little') for word in words)
    embedded = plistlib.loads(data.rstrip(b'\0'))
    application = embedded.get('application-identifier', '')
    assert application.endswith('.' + identifier), 'Missing Xcode-generated simulator identity'
    assert application in embedded.get('keychain-access-groups', []), 'Missing embedded default private Keychain access group'
    (diagnostics / f'embedded-{architecture}.plist').write_bytes(plistlib.dumps(embedded))
subprocess.run(['/usr/bin/codesign', '--verify', '--deep', '--strict', str(app)], check=True)
print('PASS Xcode ad-hoc signature and embedded simulator Keychain identities for every executable slice; actual XCTest remains required.')
