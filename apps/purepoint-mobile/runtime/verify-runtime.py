#!/usr/bin/env python3
"""Validate every advertised native runtime slice in a staged build artifact."""
import argparse
import json
from pathlib import Path
import subprocess
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--app', required=True)
args = parser.parse_args()
app = Path(args.app).resolve()
base = app / 'Contents/Resources/PointGuard'
manifest = json.loads((base / 'runtime-manifest.json').read_text())
arches = ['arm64', 'x64'] if manifest['architecture'] == 'universal' else [manifest['architecture']]
assert manifest['schemaVersion'] == manifest['contractVersion'] == 1
assert manifest['piVersion'] == '1.1.0'
def verify(file, architectures):
    assert file.is_file(), f'Missing native runtime file: {file}'
    for architecture in architectures:
        subprocess.run(['/usr/bin/lipo',str(file),'-verify_arch','x86_64' if architecture == 'x64' else architecture],check=True)
for name in ['node', 'pu', 'lockHelper']:
    verify((base / manifest['paths'][name]).resolve(), arches)
for arch in arches:
    verify(base / f'node_modules/@esbuild/darwin-{arch}/bin/esbuild', [arch])
    verify(base / f'node_modules/@earendil-works/pi-tui/native/darwin/prebuilds/darwin-{arch}/darwin-platform.node', [arch])
print('PASS advertised helper, esbuild and Pi native addon architectures: ' + ','.join(arches))
