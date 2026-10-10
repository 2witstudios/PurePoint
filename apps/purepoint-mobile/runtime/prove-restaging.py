#!/usr/bin/env python3
"""Verify signed incremental build output, never installed apps or production signatures."""
import argparse
import importlib.util
from pathlib import Path
import plistlib
import shutil
import subprocess
import tempfile
import os
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--app', required=True)
parser.add_argument('--pu', required=True)
parser.add_argument('--lock-helper')
parser.add_argument('--architecture', required=True)
args = parser.parse_args()
app = Path(args.app).resolve()
roots = [Path(tempfile.gettempdir()).resolve(), Path('/tmp').resolve()]
if os.environ.get('RUNNER_TEMP'):
    roots.append(Path(os.environ['RUNNER_TEMP']).resolve())
if not any(root in app.parents for root in roots):
    raise ValueError('Proof artifacts must be in the temporary directory.')
script = Path(__file__).with_name('package-runtime.py')
spec = importlib.util.spec_from_file_location('package_runtime', script)
package = importlib.util.module_from_spec(spec)
spec.loader.exec_module(package)
main = app / 'Contents/MacOS/PointGuardProof'
main.parent.mkdir(exist_ok=True)
shutil.copyfile(args.pu, main)
main.chmod(0o755)
(app / 'Contents/Info.plist').write_bytes(plistlib.dumps(dict(CFBundleExecutable='PointGuardProof',
    CFBundleIdentifier='com.purepoint.runtime-proof', CFBundleName='PointGuardProof',
    CFBundlePackageType='APPL',CFBundleVersion='1',CFBundleShortVersionString='1.0')))
def sign():
    subprocess.run(['python3',str(script.with_name('sign-runtime.py')),'--app',str(app),'--identity','-'],check=True)
    subprocess.run(['/usr/bin/codesign','--force','--sign','-','--options','runtime',str(app)],check=True)
    subprocess.run(['/usr/bin/codesign','--verify','--deep','--strict',str(app)],check=True)
sign()
# A subsequent build must explicitly own the output before replacing its old signature.
try:
    package.stage(app,args.pu,args.architecture,Path(tempfile.gettempdir())/'pointguard-node-cache',lock_helper=args.lock_helper)
except ValueError as error:
    assert 'explicit' in str(error)
else:
    raise AssertionError('Signed artifact was restaged without explicit ownership.')
package.stage(app,args.pu,args.architecture,Path(tempfile.gettempdir())/'pointguard-node-cache',True,app.parent,args.lock_helper)
sign()
print('PASS repeated signed build-artifact staging; nested and outer ad-hoc seal verified.')
