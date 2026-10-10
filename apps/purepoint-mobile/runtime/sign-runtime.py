#!/usr/bin/env python3
"""Inside-out ad-hoc signing proof on CI artifacts; no production identity/publishing."""
from pathlib import Path
import subprocess
import argparse
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--app', required=True)
parser.add_argument('--identity', required=True, help='Explicit signing identity; CI uses - for ad-hoc, owner release supplies Developer ID')
args = parser.parse_args()
app = Path(args.app).resolve()
identity = args.identity
resources = app / 'Contents/Resources/PointGuard'
entitlements = Path(__file__).with_name('node-entitlements.plist')

def run(*args):
    subprocess.run(args, check=True)

for file in resources.rglob('*'):
    if not file.is_file():
        continue
    with file.open('rb') as stream:
        magic = stream.read(4)
    if magic in [b'\xcf\xfa\xed\xfe', b'\xce\xfa\xed\xfe', b'\xfe\xed\xfa\xcf', b'\xca\xfe\xba\xbe']:
        run('/usr/bin/codesign', '--force', '--sign', identity, '--options', 'runtime', str(file))
        run('/usr/bin/codesign', '--verify', '--strict', str(file))
for name in ['pu', 'point-guard-node']:
    file = app / 'Contents/Helpers' / name
    args = ['/usr/bin/codesign', '--force', '--sign', identity, '--options', 'runtime']
    if name == 'point-guard-node':
        args += ['--entitlements', str(entitlements)]
    run(*args, str(file))
    run('/usr/bin/codesign', '--verify', '--strict', str(file))
run('/usr/bin/codesign', '--display', '--entitlements', ':-', str(app / 'Contents/Helpers/point-guard-node'))
# No identity discovery, credential access, notarization or publication.
# The caller signs the outer app after staging nested code. --deep is verification only.
