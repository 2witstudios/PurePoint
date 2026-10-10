#!/usr/bin/env python3
"""Inspect a temporary simulator test host, repairing only missing ad-hoc access identity."""
import argparse
from pathlib import Path
import os
import plistlib
import subprocess
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--app', required=True)
parser.add_argument('--diagnostics', required=True)
args = parser.parse_args()
app = Path(args.app).resolve()
root = Path(os.environ['RUNNER_TEMP']).resolve()
assert root in app.parents, 'Only current CI temporary artifacts may be signed'
info = plistlib.loads((app / 'Info.plist').read_bytes())
assert 'iPhoneSimulator' in info.get('CFBundleSupportedPlatforms', []), 'Refuse non-simulator artifacts'
identifier = info['CFBundleIdentifier']
diagnostics = Path(args.diagnostics)
diagnostics.mkdir(parents=True, exist_ok=True)
def entitlements():
    result = subprocess.run(['/usr/bin/codesign','--display','--entitlements',':-',str(app)],capture_output=True,check=True)
    (diagnostics / 'codesign-stderr.txt').write_bytes(result.stderr)
    return plistlib.loads(result.stdout) if result.stdout.strip() else {}
current = entitlements()
(diagnostics / 'original-entitlements.plist').write_bytes(plistlib.dumps(current))
if not current.get('application-identifier'):
    # Simulator-only ad-hoc signing has no production Team ID or provisioning profile.
    # Use the actual existing bundle identifier for the app's default private Keychain group.
    current['application-identifier'] = identifier
    current['keychain-access-groups'] = [identifier]
    file = diagnostics / 'simulator-entitlements.plist'
    file.write_bytes(plistlib.dumps(current))
    subprocess.run(['/usr/bin/codesign','--force','--sign','-','--entitlements',str(file),str(app)],check=True)
current = entitlements()
assert current.get('application-identifier'), 'Simulator must have a signed application identity'
assert current.get('keychain-access-groups') or current['application-identifier'], 'Default private access identity missing'
(diagnostics / 'verified-entitlements.plist').write_bytes(plistlib.dumps(current))
subprocess.run(['/usr/bin/codesign','--verify','--deep','--strict',str(app)],check=True)
print('PASS simulator ad-hoc signature and default Keychain application identity; actual XCTest remains required.')
