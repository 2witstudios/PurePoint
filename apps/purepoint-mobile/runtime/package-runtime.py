#!/usr/bin/env python3
"""Stage pinned runtime into an unsigned build artifact; never install or publish."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tarfile
import tempfile
import urllib.request

SOURCE = Path(__file__).resolve().parents[1]
PIN = json.loads((SOURCE / 'runtime/node-release.json').read_text())

def run(*args, **kwargs):
    subprocess.run(args, check=True, **kwargs)

def stage(app, pu, architecture, cache, replace_build_artifact=False, build_root=None):
    app, pu, cache = Path(app).resolve(), Path(pu).resolve(), Path(cache).resolve()
    if app.is_relative_to('/Applications') or app.is_relative_to('/System/Applications') or app.is_relative_to(Path.home() / 'Applications'):
        raise ValueError('Cannot stage into installed application directories.')
    if not app.name.endswith('.app'):
        raise ValueError('Destination must be a build artifact .app.')
    if not pu.is_file():
        raise ValueError('Provide the freshly built artifact pu binary, never an installed binary.')
    if app.exists() and (app / 'Contents/_CodeSignature').exists():
        if not replace_build_artifact or not build_root:
            raise ValueError('Signed output requires explicit --replace-build-artifact and --build-root.')
        artifact_root = Path(build_root).resolve()
        if artifact_root not in app.parents or app.is_relative_to('/Applications') or app.is_relative_to(Path.home() / 'Applications'):
            raise ValueError('Restaging is limited to the explicit build artifact root, never installed apps.')
        shutil.rmtree(app / 'Contents/_CodeSignature')
    if '.app' not in app.name:
        raise ValueError('Destination must be a build artifact .app.')
    app.mkdir(parents=True, exist_ok=True)
    helpers = app / 'Contents/Helpers'
    target = app / 'Contents/Resources/PointGuard'
    helpers.mkdir(parents=True, exist_ok=True)
    if target.exists():
        shutil.rmtree(target)
    target.mkdir(parents=True)
    cache.mkdir(parents=True, exist_ok=True)
    arches = ['arm64', 'x64'] if architecture == 'universal' else [architecture]
    with tempfile.TemporaryDirectory(prefix='pointguard-package-') as folder:
        temp = Path(folder)
        nodes = []
        for arch in arches:
            pin = PIN['archives'][arch]
            archive = cache / pin['name']
            if not archive.exists():
                download = archive.with_suffix('.download')
                try:
                    urllib.request.urlretrieve(f"https://nodejs.org/dist/v{PIN['version']}/{pin['name']}", download)
                    if hashlib.sha256(download.read_bytes()).hexdigest() != pin['sha256']:
                        raise ValueError('Pinned Node checksum mismatch')
                    os.replace(download, archive)
                finally:
                    download.unlink(missing_ok=True)
            if hashlib.sha256(archive.read_bytes()).hexdigest() != pin['sha256']:
                raise ValueError('Cached Node checksum mismatch')
            with tarfile.open(archive) as tar:
                member = tar.getmember(pin['name'].removesuffix('.tar.gz') + '/bin/node')
                output = temp / f'node-{arch}'
                with tar.extractfile(member) as stream, output.open('wb') as file:
                    shutil.copyfileobj(stream, file)
                output.chmod(0o755)
                nodes.append(str(output))
        if len(nodes) == 2:
            run('/usr/bin/lipo', '-create', *nodes, '-output', str(helpers / 'point-guard-node'))
        else:
            shutil.copy2(nodes[0], helpers / 'point-guard-node')
        shutil.copyfile(pu, helpers / 'pu')
        (helpers / 'pu').chmod(0o755)
        # All production transitive npm dependencies and package resources come from the lock.
        npm = shutil.which('npm')
        if not npm:
            raise ValueError('Build machine needs npm; installed app does not.')
        for name in ['package.json', 'package-lock.json']:
            shutil.copy2(SOURCE / name, temp / name)
        run(npm, 'ci', '--omit=dev', '--ignore-scripts', '--no-audit', '--no-fund', cwd=temp)
        shutil.copytree(temp / 'node_modules', target / 'node_modules', symlinks=False)
        # Remove foreign native addons; keep each advertised Darwin architecture.
        for platform in ['win32', 'linux']:
            shutil.rmtree(target / 'node_modules/@earendil-works/pi-tui/native' / platform, ignore_errors=True)
        for arch in ['arm64', 'x64']:
            if arch not in arches:
                shutil.rmtree(target / f'node_modules/@earendil-works/pi-tui/native/darwin/prebuilds/darwin-{arch}', ignore_errors=True)
        shutil.copytree(SOURCE / 'bridge', target / 'bridge', ignore=shutil.ignore_patterns('*.test.js'))
        shutil.copytree(SOURCE / 'docs', target / 'docs')
        shutil.copytree(SOURCE / 'support', target / 'support')
        shutil.copy2(SOURCE / 'package.json', target / 'package.json')
        sha = subprocess.check_output(['git', '-C', str(SOURCE), 'rev-parse', 'HEAD'], text=True).strip()
        manifest = dict(schemaVersion=1, contractVersion=1, piVersion='1.1.0', nodeVersion=PIN['version'],
                        architecture=architecture, sourceSHA=sha,
                        paths=dict(node='../../Helpers/point-guard-node', pu='../../Helpers/pu',
                                   entry='bridge/main.js', instructions='docs/point-guard.md',
                                   skills=['support/pu/SKILL.md', 'support/pu-cli/SKILL.md']))
        (target / 'runtime-manifest.json').write_text(json.dumps(manifest, indent=2) + '\n')
    return target

if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--app', required=True)
    parser.add_argument('--pu', required=True)
    parser.add_argument('--architecture', choices=['arm64','x64','universal'], required=True)
    parser.add_argument('--replace-build-artifact', action='store_true')
    parser.add_argument('--build-root', help='Explicit build directory that owns a signed incremental output')
    parser.add_argument('--cache', default=os.path.join(tempfile.gettempdir(), 'pointguard-node-cache'))
    options = parser.parse_args()
    stage(options.app, options.pu, options.architecture, options.cache, options.replace_build_artifact, options.build_root)
