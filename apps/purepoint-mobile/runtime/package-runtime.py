#!/usr/bin/env python3
"""Stage pinned runtime into an unsigned build artifact; never install or publish."""
import argparse
import base64
import hashlib
import http.client
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tarfile
import tempfile
import time
import urllib.error
import urllib.request

SOURCE = Path(__file__).resolve().parents[1]
PIN = json.loads((SOURCE / 'runtime/node-release.json').read_text())

def run(*args, **kwargs):
    subprocess.run(args, check=True, **kwargs)

def download_archive(url, destination):
    """Retry transient network failures without publishing partial cache entries."""
    for attempt in range(3):
        try:
            with urllib.request.urlopen(url, timeout=30) as response, destination.open('wb') as output:
                shutil.copyfileobj(response, output)
                expected = getattr(response, 'headers', {}).get('Content-Length')
                if expected is not None and output.tell() != int(expected):
                    raise urllib.error.ContentTooShortError('Incomplete runtime archive', None)
            return
        except (urllib.error.URLError, TimeoutError, ConnectionError, http.client.IncompleteRead) as error:
            destination.unlink(missing_ok=True)
            if isinstance(error, urllib.error.HTTPError) and error.code not in (408, 429) and error.code < 500:
                raise
            if attempt == 2:
                raise
            print(f'Transient runtime download failure; retry {attempt + 1}/2: {error}', file=sys.stderr)
            time.sleep(attempt + 1)

def complete_darwin_graph(modules, arches, cache):
    # npm selects optional binaries for the build host. Restore every advertised
    # Darwin architecture from exact lockfile URLs with its recorded integrity.
    complete_platform_graph(modules, 'darwin', arches, cache)

def wanted_native(entry, os_name, arches, libc=None):
    """Whether a platform-specific lock entry belongs to the target OS/arch (and libc)."""
    if any(value.startswith('!') for field in ('os', 'cpu', 'libc') for value in entry.get(field, [])):
        raise ValueError('Negated platform constraints are not supported')
    if os_name not in entry.get('os', []) or not set(entry.get('cpu', [])) & set(arches):
        return False
    return libc is None or not entry.get('libc') or libc in entry['libc']

def complete_platform_graph(modules, os_name, arches, cache, prune_foreign=False, libc=None):
    """Keep exactly the target platform's optional natives, restored from the lock.

    Without `prune_foreign` (the darwin app path) only entries for `os_name` are
    touched. With it (Linux payloads, possibly built on another host OS) every
    platform-specific package that is not for the target is removed, so a host
    native never ships in the payload.
    """
    lock = json.loads((SOURCE / 'package-lock.json').read_text())
    for location, entry in lock['packages'].items():
        if entry.get('dev'):
            continue
        if prune_foreign:
            if not entry.get('os') and not entry.get('cpu'):
                continue
            if not location.startswith('node_modules/') or '..' in Path(location).parts:
                raise ValueError('Unsafe native package lock path')
        elif os_name not in entry.get('os', []):
            continue
        destination = modules.parent / location
        wanted = wanted_native(entry, os_name, arches, libc) if prune_foreign else bool(set(entry.get('cpu', [])) & set(arches))
        if not wanted:
            shutil.rmtree(destination, ignore_errors=True)
            continue
        if destination.exists():
            continue
        if not location.startswith('node_modules/') or '..' in Path(location).parts:
            raise ValueError('Unsafe native package lock path')
        algorithm, expected = entry['integrity'].split('-', 1)
        if algorithm != 'sha512' or not entry['resolved'].startswith('https://registry.npmjs.org/'):
            raise ValueError('Unsupported native package integrity or source')
        archive = cache / (hashlib.sha256(entry['resolved'].encode()).hexdigest() + '.tgz')
        if not archive.exists():
            with tempfile.NamedTemporaryFile(dir=cache, suffix='.download', delete=False) as file:
                download = Path(file.name)
            try:
                download_archive(entry['resolved'], download)
                actual = base64.b64encode(hashlib.sha512(download.read_bytes()).digest()).decode()
                if actual != expected:
                    raise ValueError('Locked native package checksum mismatch')
                os.replace(download, archive)
            finally:
                download.unlink(missing_ok=True)
        actual = base64.b64encode(hashlib.sha512(archive.read_bytes()).digest()).decode()
        if actual != expected:
            raise ValueError('Locked native package checksum mismatch')
        destination.mkdir(parents=True)
        with tarfile.open(archive) as tar:
            for member in tar.getmembers():
                parts = Path(member.name).parts
                if not parts or parts[0] != 'package' or '..' in parts or member.issym() or member.islnk():
                    raise ValueError('Unsafe locked package archive')
                output = destination.joinpath(*parts[1:])
                if member.isdir():
                    output.mkdir(parents=True, exist_ok=True)
                elif member.isfile():
                    output.parent.mkdir(parents=True, exist_ok=True)
                    with tar.extractfile(member) as stream, output.open('wb') as file:
                        shutil.copyfileobj(stream, file)
                    output.chmod(member.mode & 0o777)
                else:
                    raise ValueError('Unsupported locked package member')

def stage(app, pu, architecture, cache, replace_build_artifact=False, build_root=None, lock_helper=None):
    app, pu, cache = Path(app).resolve(), Path(pu).resolve(), Path(cache).resolve()
    if app.is_relative_to('/Applications') or app.is_relative_to('/System/Applications') or app.is_relative_to(Path.home() / 'Applications'):
        raise ValueError('Cannot stage into installed application directories.')
    if not app.name.endswith('.app'):
        raise ValueError('Destination must be a build artifact .app.')
    lock_helper = Path(lock_helper).resolve() if lock_helper else pu.with_name('pu-point-guard-lock')
    if not lock_helper.is_file():
        raise ValueError('Provide the freshly built artifact pu-point-guard-lock helper.')
    if not pu.is_file():
        raise ValueError('Provide the freshly built artifact pu binary, never an installed binary.')
    # Reject incomplete or edited imports before replacing a build artifact.
    run(sys.executable, str(SOURCE / 'support/pointguard-workflows/tools/import-workflows.py'), '--verify')
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
                    download_archive(f"https://nodejs.org/dist/v{PIN['version']}/{pin['name']}", download)
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
        shutil.copyfile(lock_helper, helpers / 'point-guard-lock')
        (helpers / 'point-guard-lock').chmod(0o755)
        # All production transitive npm dependencies and package resources come from the lock.
        npm = shutil.which('npm')
        if not npm:
            raise ValueError('Build machine needs npm; installed app does not.')
        for name in ['package.json', 'package-lock.json']:
            shutil.copy2(SOURCE / name, temp / name)
        run(npm, 'ci', '--omit=dev', '--ignore-scripts', '--no-audit', '--no-fund', cwd=temp)
        shutil.copytree(temp / 'node_modules', target / 'node_modules', symlinks=False)
        complete_darwin_graph(target / 'node_modules', arches, cache)
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
                        paths=dict(node='../../Helpers/point-guard-node', pu='../../Helpers/pu', lockHelper='../../Helpers/point-guard-lock',
                                   entry='bridge/main.js', instructions='docs/point-guard.md',
                                   workflowPackage='support/pointguard-workflows',
                                   skills=['support/pu/SKILL.md', 'support/pu-cli/SKILL.md']))
        (target / 'runtime-manifest.json').write_text(json.dumps(manifest, indent=2) + '\n')
    return target

# ---------------------------------------------------------------------------
# Linux bridge payload (`--layout dir`): a plain versioned directory and its
# release tarball. Nothing here is used by the darwin app path above.

# Node.js target -> (npm cpu, release asset arch, ELF e_machine).
LINUX_TARGETS = {
    'linux-x64': ('x64', 'x86_64', 0x3E),
    'linux-arm64': ('arm64', 'aarch64', 0xB7),
}
# Official nodejs.org Linux builds need glibc >= 2.28 (LPH-001).
MIN_GLIBC = '2.28'
VERSION_PATTERN = re.compile(r'[0-9]+\.[0-9]+\.[0-9]+')
# Files `pu phone` executes or reads before trusting a payload.
REQUIRED_PAYLOAD_FILES = ('runtime-manifest.json', 'bin/node', 'bin/point-guard-lock', 'bridge/main.js')
# Same character set install.sh accepts for every tarball entry.
SAFE_ENTRY_NAME = re.compile(r'[A-Za-z0-9._@+=-]+')

def workspace_version(cargo_toml):
    """The `[workspace.package] version` of the root Cargo.toml text."""
    section = re.search(r'^\[workspace\.package\][ \t]*$(.*?)(?=^\[|\Z)', cargo_toml, re.M | re.S)
    match = section and re.search(r'^version\s*=\s*"([^"]+)"[ \t]*$', section.group(1), re.M)
    if not match:
        raise ValueError('Cargo.toml has no [workspace.package] version')
    return match.group(1)

def check_versions(pu_version, release_version, cargo_toml):
    """The payload must name exactly the `pu` it ships with (lockstep, LPH-002)."""
    for value in (pu_version, release_version):
        if not VERSION_PATTERN.fullmatch(value or ''):
            raise ValueError(f'Version must be plain X.Y.Z, got {value!r}')
    crate = workspace_version(cargo_toml)
    if pu_version != crate:
        raise ValueError(f'--pu-version {pu_version} differs from the workspace pu crate version {crate}')

def payload_name(release_version, target):
    return f'purepoint-pointguard-{release_version}-{LINUX_TARGETS[target][1]}-linux-gnu'

def elf_machine(header):
    """e_machine of a little-endian 64-bit ELF header, else None."""
    if len(header) < 20 or header[:4] != b'\x7fELF' or header[4] != 2 or header[5] != 1:
        return None
    return int.from_bytes(header[18:20], 'little')

def require_elf(path, target, what):
    with open(path, 'rb') as file:
        machine = elf_machine(file.read(64))
    if machine != LINUX_TARGETS[target][2]:
        raise ValueError(f'{what} is not a {target} ELF executable: {path}')

def linux_manifest(pu_version, target, node_version, source_sha):
    """Payload-relative paths only; `pu` is the installed one beside pu-engine."""
    return dict(schemaVersion=1, contractVersion=1, layout='linux', puVersion=pu_version,
                piVersion='1.1.0', nodeVersion=node_version, target=target,
                architecture=LINUX_TARGETS[target][0], libc='glibc', minGlibc=MIN_GLIBC,
                sourceSHA=source_sha,
                paths=dict(node='bin/node', lockHelper='bin/point-guard-lock',
                           entry='bridge/main.js', instructions='docs/point-guard.md',
                           workflowPackage='support/pointguard-workflows',
                           skills=['support/pu/SKILL.md', 'support/pu-cli/SKILL.md']))

def keep_native_prebuilds(native, os_name, arch):
    """Keep only `<os>/prebuilds/<os>-<arch>` under a package's native/ directory."""
    if not native.is_dir():
        raise ValueError(f'Missing native addon directory {native}')
    for platform in native.iterdir():
        if platform.is_dir() and not platform.is_symlink() and platform.name != os_name:
            shutil.rmtree(platform)
    prebuilds = native / os_name / 'prebuilds'
    if not (prebuilds / f'{os_name}-{arch}').is_dir():
        raise ValueError(f'Missing {os_name}-{arch} native prebuild in {native}')
    for build in prebuilds.iterdir():
        if build.name != f'{os_name}-{arch}':
            if build.is_dir() and not build.is_symlink():
                shutil.rmtree(build)
            else:
                build.unlink()

def normalize_tree(root):
    """Owner-writable only: dirs 0755, files 0755 when executable else 0644; no links."""
    root = Path(root)
    for directory, folders, files in os.walk(root):
        for name in folders + files:
            path = Path(directory) / name
            if path.is_symlink():
                raise ValueError(f'Payload must not contain symlinks: {path}')
            if path.is_dir():
                path.chmod(0o755)
            elif path.is_file():
                path.chmod(0o755 if path.stat().st_mode & 0o111 else 0o644)
            else:
                raise ValueError(f'Payload must contain only files and directories: {path}')
    root.chmod(0o755)

def payload_problems(root):
    """Everything `pu phone` or install.sh would refuse, as messages (empty when sound)."""
    root = Path(root)
    problems = []
    for directory, folders, files in os.walk(root):
        for name in folders + files:
            path = Path(directory) / name
            if not SAFE_ENTRY_NAME.fullmatch(name):
                # install.sh parses `tar -tv` listings; keep every name unambiguous.
                problems.append(f'unsafe name: {path.relative_to(root)!r}')
            if path.is_symlink():
                problems.append(f'symlink: {path.relative_to(root)}')
            elif path.lstat().st_mode & 0o022:
                problems.append(f'group/other-writable: {path.relative_to(root)}')
            elif not (path.is_dir() or path.is_file()):
                problems.append(f'special file: {path.relative_to(root)}')
    for relative in REQUIRED_PAYLOAD_FILES:
        path = root / relative
        if path.is_symlink() or not path.is_file():
            problems.append(f'missing: {relative}')
        elif relative.startswith('bin/') and not os.access(path, os.X_OK):
            problems.append(f'not executable: {relative}')
    return problems

def write_tarball(root, name, output, mtime):
    """Deterministic gzip tarball: sorted entries under `name/`, root-owned, fixed mtime."""
    import gzip
    root = Path(root)
    entries = [root] + sorted(root.rglob('*'), key=lambda p: p.relative_to(root).as_posix())
    with open(output, 'wb') as raw, gzip.GzipFile(fileobj=raw, mode='wb', mtime=0, filename='') as compressed, \
            tarfile.open(fileobj=compressed, mode='w', format=tarfile.PAX_FORMAT) as tar:
        for path in entries:
            relative = path.relative_to(root).as_posix()
            info = tar.gettarinfo(str(path), arcname=name if relative == '.' else f'{name}/{relative}')
            if not (info.isdir() or info.isfile()):
                raise ValueError(f'Unsupported payload entry: {path}')
            info.uid = info.gid = 0
            info.uname = info.gname = ''
            info.mtime = mtime
            info.mode &= 0o755
            if info.isfile():
                with path.open('rb') as stream:
                    tar.addfile(info, stream)
            else:
                tar.addfile(info)

def verified_node_archive(pin, cache):
    """The pinned official Node archive, downloaded once and checked on every use."""
    archive = cache / pin['name']
    if not archive.exists():
        download = archive.with_suffix('.download')
        try:
            download_archive(f"https://nodejs.org/dist/v{PIN['version']}/{pin['name']}", download)
            if hashlib.sha256(download.read_bytes()).hexdigest() != pin['sha256']:
                raise ValueError('Pinned Node checksum mismatch')
            os.replace(download, archive)
        finally:
            download.unlink(missing_ok=True)
    if hashlib.sha256(archive.read_bytes()).hexdigest() != pin['sha256']:
        raise ValueError('Cached Node checksum mismatch')
    return archive

def stage_linux(out, target, cache, lock_helper, pu_version, release_version=None):
    """Build `<out>/<name>/` plus `<out>/<name>.tar.gz` and `.sha256` for one Linux arch."""
    if target not in LINUX_TARGETS:
        raise ValueError(f'Unknown Linux target {target!r}')
    release_version = release_version or pu_version
    check_versions(pu_version, release_version, (SOURCE.parents[1] / 'Cargo.toml').read_text())
    arch = LINUX_TARGETS[target][0]
    out, cache, lock_helper = Path(out).resolve(), Path(cache).resolve(), Path(lock_helper).resolve()
    if not lock_helper.is_file():
        raise ValueError('Provide the freshly built pu-point-guard-lock helper for the target.')
    require_elf(lock_helper, target, 'Lock helper')
    name = payload_name(release_version, target)
    target_dir, tarball, checksum = out / name, out / f'{name}.tar.gz', out / f'{name}.tar.gz.sha256'
    for path in (target_dir, tarball, checksum):
        if path.exists() or path.is_symlink():
            raise ValueError(f'Refusing to replace existing output: {path}')
    # Reject incomplete or edited imports before building anything.
    run(sys.executable, str(SOURCE / 'support/pointguard-workflows/tools/import-workflows.py'), '--verify')
    npm = shutil.which('npm')
    if not npm:
        raise ValueError('Build machine needs npm; the payload does not.')
    out.mkdir(parents=True, exist_ok=True)
    cache.mkdir(parents=True, exist_ok=True)
    pin = PIN['archives'][target]
    archive = verified_node_archive(pin, cache)
    with tempfile.TemporaryDirectory(prefix='.pointguard-linux-', dir=out) as folder:
        temp = Path(folder)
        staging = temp / name
        (staging / 'bin').mkdir(parents=True)
        with tarfile.open(archive) as tar:
            member = tar.getmember(pin['name'].removesuffix('.tar.gz') + '/bin/node')
            if not member.isfile():
                raise ValueError('Pinned Node archive has no regular bin/node')
            with tar.extractfile(member) as stream, (staging / 'bin/node').open('wb') as file:
                shutil.copyfileobj(stream, file)
        (staging / 'bin/node').chmod(0o755)
        require_elf(staging / 'bin/node', target, 'Node')
        shutil.copyfile(lock_helper, staging / 'bin/point-guard-lock')
        (staging / 'bin/point-guard-lock').chmod(0o755)
        # All production transitive npm dependencies come from the lock, scripts off.
        install = temp / 'npm'
        install.mkdir()
        for file in ['package.json', 'package-lock.json']:
            shutil.copy2(SOURCE / file, install / file)
        run(npm, 'ci', '--omit=dev', '--ignore-scripts', '--no-audit', '--no-fund', cwd=install)
        shutil.copytree(install / 'node_modules', staging / 'node_modules', symlinks=False)
        # The build host may be another OS/arch: drop every foreign optional
        # native and restore the target's from exact lockfile integrity.
        complete_platform_graph(staging / 'node_modules', 'linux', [arch], cache, prune_foreign=True, libc='glibc')
        keep_native_prebuilds(staging / 'node_modules/@earendil-works/pi-tui/native', 'linux', arch)
        shutil.copytree(SOURCE / 'bridge', staging / 'bridge', ignore=shutil.ignore_patterns('*.test.js'))
        shutil.copytree(SOURCE / 'docs', staging / 'docs')
        shutil.copytree(SOURCE / 'support', staging / 'support')
        shutil.copy2(SOURCE / 'package.json', staging / 'package.json')
        sha = subprocess.check_output(['git', '-C', str(SOURCE), 'rev-parse', 'HEAD'], text=True).strip()
        committed = int(subprocess.check_output(['git', '-C', str(SOURCE), 'log', '-1', '--format=%ct', 'HEAD'], text=True).strip())
        manifest = linux_manifest(pu_version, target, PIN['version'], sha)
        (staging / 'runtime-manifest.json').write_text(json.dumps(manifest, indent=2) + '\n')
        normalize_tree(staging)
        problems = payload_problems(staging)
        if problems:
            raise ValueError('Unsafe payload: ' + '; '.join(problems[:10]))
        partial = temp / f'{name}.tar.gz'
        write_tarball(staging, name, partial, int(os.environ.get('SOURCE_DATE_EPOCH', committed)))
        digest = hashlib.sha256(partial.read_bytes()).hexdigest()
        os.replace(staging, target_dir)
        os.replace(partial, tarball)
    checksum.write_text(f'{digest}  {name}.tar.gz\n')
    return target_dir, tarball

if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--layout', choices=['app', 'dir'], default='app',
                        help='app: macOS build artifact .app (default); dir: Linux bridge payload directory and tarball')
    parser.add_argument('--app', help='app layout: destination build artifact .app')
    parser.add_argument('--pu', help='app layout: freshly built artifact pu')
    parser.add_argument('--lock-helper', help='Freshly built pu-point-guard-lock artifact; app layout defaults to the pu sibling')
    parser.add_argument('--architecture', choices=['arm64','x64','universal'], help='app layout target')
    parser.add_argument('--target', choices=sorted(LINUX_TARGETS), help='dir layout target')
    parser.add_argument('--out', help='dir layout: output directory for the payload and tarball')
    parser.add_argument('--pu-version', help='dir layout: version of the pu it ships with (must equal the workspace crate version)')
    parser.add_argument('--release-version', help='dir layout: release version in the asset name (default: --pu-version)')
    parser.add_argument('--print-pu-version', action='store_true', help='Print the workspace pu crate version and exit')
    parser.add_argument('--replace-build-artifact', action='store_true')
    parser.add_argument('--build-root', help='Explicit build directory that owns a signed incremental output')
    parser.add_argument('--cache', default=os.path.join(tempfile.gettempdir(), 'pointguard-node-cache'))
    options = parser.parse_args()
    if options.print_pu_version:
        print(workspace_version((SOURCE.parents[1] / 'Cargo.toml').read_text()))
    elif options.layout == 'dir':
        if not (options.target and options.out and options.lock_helper and options.pu_version):
            parser.error('--layout dir needs --target, --out, --lock-helper and --pu-version')
        if options.app or options.pu or options.architecture or options.replace_build_artifact or options.build_root:
            parser.error('--app, --pu, --architecture, --replace-build-artifact and --build-root apply only to --layout app')
        directory, tarball = stage_linux(options.out, options.target, options.cache, options.lock_helper,
                                         options.pu_version, options.release_version)
        print(directory)
        print(tarball)
    else:
        if not (options.app and options.pu and options.architecture):
            parser.error('--layout app needs --app, --pu and --architecture')
        if options.target or options.out or options.pu_version or options.release_version:
            parser.error('--target, --out, --pu-version and --release-version apply only to --layout dir')
        stage(options.app, options.pu, options.architecture, options.cache, options.replace_build_artifact, options.build_root, options.lock_helper)
