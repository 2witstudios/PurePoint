"""Disposable offline regression for interrupted native-package downloads."""
import base64
import hashlib
import importlib.util
import io
import json
from pathlib import Path
import shutil
import tarfile
import tempfile
import urllib.error
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('package_runtime', Path(__file__).with_name('package-runtime.py'))
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)
with tempfile.TemporaryDirectory(prefix='pointguard-cache-', dir='/tmp') as folder:
    root = Path(folder)
    cache = root / 'cache'; cache.mkdir()
    source = root / 'source'; source.mkdir()
    stream = io.BytesIO()
    with tarfile.open(fileobj=stream, mode='w:gz') as archive:
        member = tarfile.TarInfo('package/bin/tool'); member.size = 5
        archive.addfile(member, io.BytesIO(b'valid'))
    payload = stream.getvalue()
    entry = {'os': ['darwin'], 'cpu': ['x64'], 'resolved': 'https://registry.npmjs.org/fixture/tool.tgz',
             'integrity': 'sha512-' + base64.b64encode(hashlib.sha512(payload).digest()).decode()}
    (source / 'package-lock.json').write_text(json.dumps({'packages': {'node_modules/fixture': entry}}))
    modules = root / 'node_modules'
    def interrupted(url, destination):
        Path(destination).write_bytes(b'partial')
        raise OSError('fixture download interrupted')
    def valid(url, destination): Path(destination).write_bytes(payload)
    with patch.object(module, 'SOURCE', source), patch.object(module, 'download_archive', interrupted):
        try: module.complete_darwin_graph(modules, ['x64'], cache)
        except OSError: pass
        else: raise AssertionError('Interrupted download accepted')
    assert list(cache.iterdir()) == [], 'Partial download must never become a cache entry'
    with patch.object(module, 'SOURCE', source), patch.object(module, 'download_archive', valid):
        module.complete_darwin_graph(modules, ['x64'], cache)
    assert (modules / 'fixture/bin/tool').read_bytes() == b'valid'
    assert len(list(cache.iterdir())) == 1
    # A successful HTTP response with wrong bytes must still fail integrity checks.
    shutil.rmtree(modules)
    for archive in cache.iterdir(): archive.unlink()
    with patch.object(module, 'SOURCE', source), \
         patch.object(module.urllib.request, 'urlopen', return_value=io.BytesIO(b'corrupt')) as request:
        try: module.complete_darwin_graph(modules, ['x64'], cache)
        except ValueError as error: assert 'checksum mismatch' in str(error)
        else: raise AssertionError('Checksum mismatch accepted')
        assert request.call_count == 1 and list(cache.iterdir()) == []
print('Interrupted native download cleanup and exact-integrity retry PASS')

with tempfile.TemporaryDirectory(prefix='pointguard-network-', dir='/tmp') as folder:
    destination = Path(folder) / 'archive.download'
    transient = urllib.error.URLError('temporary DNS failure')
    with patch.object(module.urllib.request, 'urlopen', side_effect=[transient, transient, io.BytesIO(b'pinned bytes')]) as request, \
         patch.object(module.time, 'sleep') as sleep:
        module.download_archive('https://nodejs.org/fixture', destination)
        assert destination.read_bytes() == b'pinned bytes'
        assert request.call_count == 3 and sleep.call_count == 2
        assert all(call.kwargs['timeout'] == 30 for call in request.call_args_list)
    with patch.object(module.urllib.request, 'urlopen', side_effect=transient) as request, \
         patch.object(module.time, 'sleep'):
        try: module.download_archive('https://nodejs.org/fixture', destination)
        except urllib.error.URLError: pass
        else: raise AssertionError('Persistent download failure accepted')
        assert request.call_count == 3 and not destination.exists()
    permanent = urllib.error.HTTPError('https://nodejs.org/fixture', 404, 'not found', {}, None)
    with patch.object(module.urllib.request, 'urlopen', side_effect=permanent) as request, \
         patch.object(module.time, 'sleep') as sleep:
        try: module.download_archive('https://nodejs.org/fixture', destination)
        except urllib.error.HTTPError: pass
        else: raise AssertionError('Permanent HTTP failure accepted')
        assert request.call_count == 1 and not sleep.called and not destination.exists()
    partial = io.BytesIO(b'old partial')
    partial.headers = {'Content-Length': '100'}
    with patch.object(module.urllib.request, 'urlopen', side_effect=[partial, io.BytesIO(b'complete')]) as request, \
         patch.object(module.time, 'sleep'):
        module.download_archive('https://nodejs.org/fixture', destination)
        assert request.call_count == 2 and destination.read_bytes() == b'complete'
print('Bounded transient download retry, timeout and permanent failure checks PASS')


# --- Linux bridge payload (`--layout dir`): pure functions, no network. ---
import os
import stat

assert module.workspace_version('[workspace]\nmembers = []\n\n[workspace.package]\nversion = "1.2.3"\nedition = "2024"\n\n[workspace.dependencies]\nversion = "9.9.9"\n') == '1.2.3'
for broken in ('[package]\nversion = "1.2.3"\n', '[workspace.package]\nedition = "2024"\n[x]\nversion = "1.0.0"\n'):
    try: module.workspace_version(broken)
    except ValueError: pass
    else: raise AssertionError('Missing workspace version accepted')
cargo = '[workspace.package]\nversion = "0.4.0"\n'
module.check_versions('0.4.0', None, cargo)
# Release builds compile the release version into pu, so the two must match.
module.check_versions('1.0.0', '1.0.0', cargo)
for pu_version, release in (('0.4.1', None), ('0.4.0', '1.0.0'), ('0.4.0', 'v1.0.0'), ('0.4.0/..', '0.4.0'), ('0.4', '0.4'), ('', '0.4.0')):
    try: module.check_versions(pu_version, release, cargo)
    except ValueError: pass
    else: raise AssertionError(f'Version {pu_version!r}/{release!r} accepted')
# The real workspace version is readable, so CI can pass it through.
module.workspace_version((Path(__file__).resolve().parents[3] / 'Cargo.toml').read_text())
print('Workspace pu version lockstep checks PASS')

assert module.payload_name('1.2.3', 'linux-x64') == 'purepoint-pointguard-1.2.3-x86_64-linux-gnu'
assert module.payload_name('1.2.3', 'linux-arm64') == 'purepoint-pointguard-1.2.3-aarch64-linux-gnu'
pin = json.loads(Path(__file__).with_name('node-release.json').read_text())
for target in module.LINUX_TARGETS:
    entry = pin['archives'][target]
    assert entry['name'] == f"node-v{pin['version']}-{target}.tar.gz", entry
    assert len(entry['sha256']) == 64 and int(entry['sha256'], 16) >= 0
# The darwin pins are untouched by the Linux additions.
assert pin['archives']['arm64']['name'].endswith('-darwin-arm64.tar.gz')
assert pin['archives']['x64']['name'].endswith('-darwin-x64.tar.gz')
manifest = module.linux_manifest('1.2.3', 'linux-arm64', pin['version'], 'f' * 40)
assert (manifest['schemaVersion'], manifest['layout'], manifest['puVersion']) == (1, 'linux', '1.2.3')
assert manifest['architecture'] == 'arm64' and manifest['libc'] == 'glibc' and manifest['minGlibc'] == '2.28'
assert manifest['paths']['node'] == 'bin/node' and manifest['paths']['lockHelper'] == 'bin/point-guard-lock'
assert 'pu' not in manifest['paths'], 'Linux payload uses the installed pu, never a bundled one'
assert '..' not in json.dumps(manifest) and 'Helpers' not in json.dumps(manifest)
print('Linux asset naming, Node pins and payload-relative manifest PASS')

def elf(machine): return b'\x7fELF\x02\x01\x01' + b'\0' * 11 + machine.to_bytes(2, 'little') + b'\0' * 44
assert module.elf_machine(elf(0x3E)) == 0x3E and module.elf_machine(elf(0xB7)) == 0xB7
assert module.elf_machine(b'\xcf\xfa\xed\xfe' + b'\0' * 60) is None, 'Mach-O is not ELF'
assert module.elf_machine(b'\x7fELF\x01\x01' + b'\0' * 58) is None, '32-bit ELF refused'
with tempfile.TemporaryDirectory(prefix='pointguard-elf-', dir='/tmp') as folder:
    binary = Path(folder) / 'helper'
    binary.write_bytes(elf(0xB7))
    module.require_elf(binary, 'linux-arm64', 'Lock helper')
    try: module.require_elf(binary, 'linux-x64', 'Lock helper')
    except ValueError as error: assert 'linux-x64' in str(error)
    else: raise AssertionError('Wrong-arch helper accepted')
print('Target ELF architecture checks PASS')

linux_x64 = {'os': ['linux'], 'cpu': ['x64']}
assert module.wanted_native(linux_x64, 'linux', ['x64'], 'glibc')
assert not module.wanted_native(linux_x64, 'linux', ['arm64'], 'glibc')
assert not module.wanted_native({'os': ['darwin'], 'cpu': ['x64']}, 'linux', ['x64'], 'glibc')
assert not module.wanted_native({**linux_x64, 'libc': ['musl']}, 'linux', ['x64'], 'glibc')
assert module.wanted_native({**linux_x64, 'libc': ['glibc']}, 'linux', ['x64'], 'glibc')
try: module.wanted_native({'os': ['!win32'], 'cpu': ['x64']}, 'linux', ['x64'], 'glibc')
except ValueError: pass
else: raise AssertionError('Negated platform constraint accepted')

with tempfile.TemporaryDirectory(prefix='pointguard-linux-graph-', dir='/tmp') as folder:
    root = Path(folder)
    cache = root / 'cache'; cache.mkdir()
    source = root / 'source'; source.mkdir()
    stream = io.BytesIO()
    with tarfile.open(fileobj=stream, mode='w:gz') as archive:
        member = tarfile.TarInfo('package/bin/esbuild'); member.size = 5; member.mode = 0o775
        archive.addfile(member, io.BytesIO(b'linux'))
    payload = stream.getvalue()
    integrity = 'sha512-' + base64.b64encode(hashlib.sha512(payload).digest()).decode()
    entries = {
        'node_modules/@esbuild/linux-x64': {'os': ['linux'], 'cpu': ['x64'], 'resolved': 'https://registry.npmjs.org/@esbuild/linux-x64.tgz', 'integrity': integrity, 'optional': True},
        'node_modules/@esbuild/linux-arm64': {'os': ['linux'], 'cpu': ['arm64'], 'resolved': 'https://registry.npmjs.org/@esbuild/linux-arm64.tgz', 'integrity': integrity, 'optional': True},
        'node_modules/@esbuild/darwin-arm64': {'os': ['darwin'], 'cpu': ['arm64'], 'resolved': 'https://registry.npmjs.org/@esbuild/darwin-arm64.tgz', 'integrity': integrity, 'optional': True},
        'node_modules/plain': {'resolved': 'https://registry.npmjs.org/plain.tgz', 'integrity': integrity},
    }
    (source / 'package-lock.json').write_text(json.dumps({'packages': entries}))
    modules = root / 'node_modules'
    # The build host (a Mac here) installed its own native and a plain package.
    (modules / '@esbuild/darwin-arm64/bin').mkdir(parents=True)
    (modules / '@esbuild/darwin-arm64/bin/esbuild').write_bytes(b'host')
    (modules / 'plain').mkdir()
    fetched = []
    def fixture(url, destination): fetched.append(url); Path(destination).write_bytes(payload)
    # The darwin path never touches other operating systems' entries.
    with patch.object(module, 'SOURCE', source), patch.object(module, 'download_archive', fixture):
        module.complete_darwin_graph(modules, ['arm64'], cache)
    assert not (modules / '@esbuild/linux-x64').exists() and fetched == []
    with patch.object(module, 'SOURCE', source), patch.object(module, 'download_archive', fixture):
        module.complete_platform_graph(modules, 'linux', ['x64'], cache, prune_foreign=True, libc='glibc')
    assert (modules / '@esbuild/linux-x64/bin/esbuild').read_bytes() == b'linux'
    assert not (modules / '@esbuild/darwin-arm64').exists(), 'Host native must not ship'
    assert not (modules / '@esbuild/linux-arm64').exists(), 'Other-arch native must not ship'
    assert (modules / 'plain').is_dir(), 'Platform-neutral packages stay'
    assert fetched == ['https://registry.npmjs.org/@esbuild/linux-x64.tgz']
    # Wrong bytes for the target native fail the exact lockfile integrity.
    shutil.rmtree(modules / '@esbuild/linux-x64')
    for archive in cache.iterdir(): archive.unlink()
    with patch.object(module, 'SOURCE', source), patch.object(module, 'download_archive', lambda url, destination: Path(destination).write_bytes(b'evil')):
        try: module.complete_platform_graph(modules, 'linux', ['x64'], cache, prune_foreign=True, libc='glibc')
        except ValueError as error: assert 'checksum mismatch' in str(error)
        else: raise AssertionError('Tampered target native accepted')
    entries['../escape'] = {'os': ['linux'], 'cpu': ['x64'], 'resolved': 'https://registry.npmjs.org/x.tgz', 'integrity': integrity}
    (source / 'package-lock.json').write_text(json.dumps({'packages': entries}))
    with patch.object(module, 'SOURCE', source), patch.object(module, 'download_archive', fixture):
        try: module.complete_platform_graph(modules, 'linux', ['x64'], cache, prune_foreign=True, libc='glibc')
        except ValueError as error: assert 'Unsafe' in str(error)
        else: raise AssertionError('Escaping lock path accepted')
print('Linux target-only optional natives from lockfile integrity PASS')

with tempfile.TemporaryDirectory(prefix='pointguard-native-', dir='/tmp') as folder:
    native = Path(folder) / 'native'
    for build in ('linux/prebuilds/linux-x64', 'linux/prebuilds/linux-arm64', 'darwin/prebuilds/darwin-arm64', 'win32/prebuilds/win32-x64'):
        (native / build).mkdir(parents=True)
        (native / build / 'addon.node').write_bytes(b'x')
    (native / 'napi.h').write_text('header')
    module.keep_native_prebuilds(native, 'linux', 'arm64')
    assert sorted(p.name for p in native.iterdir()) == ['linux', 'napi.h']
    assert [p.name for p in (native / 'linux/prebuilds').iterdir()] == ['linux-arm64']
    try: module.keep_native_prebuilds(native, 'linux', 'x64')
    except ValueError: pass
    else: raise AssertionError('Missing target prebuild accepted')
print('Only the target pi-tui native prebuild ships PASS')

with tempfile.TemporaryDirectory(prefix='pointguard-tree-', dir='/tmp') as folder:
    tree = Path(folder) / 'payload'
    for relative, data, mode in (('runtime-manifest.json', b'{}', 0o666), ('bin/node', b'n', 0o777), ('bin/point-guard-lock', b'l', 0o775), ('bridge/main.js', b'm', 0o664)):
        (tree / relative).parent.mkdir(parents=True, exist_ok=True)
        (tree / relative).write_bytes(data)
        (tree / relative).chmod(mode)
    (tree / 'bridge').chmod(0o777)
    assert any('group/other-writable' in p for p in module.payload_problems(tree))
    module.normalize_tree(tree)
    assert module.payload_problems(tree) == []
    assert stat.S_IMODE((tree / 'bin/node').stat().st_mode) == 0o755
    assert stat.S_IMODE((tree / 'bridge/main.js').stat().st_mode) == 0o644
    assert stat.S_IMODE((tree / 'bridge').stat().st_mode) == 0o755
    first, second = Path(folder) / 'a.tar.gz', Path(folder) / 'b.tar.gz'
    module.write_tarball(tree, 'purepoint-pointguard-1.2.3-x86_64-linux-gnu', first, 1700000000)
    os.utime(tree / 'bridge/main.js', (1, 1))
    module.write_tarball(tree, 'purepoint-pointguard-1.2.3-x86_64-linux-gnu', second, 1700000000)
    assert first.read_bytes() == second.read_bytes(), 'Payload tarball must be reproducible'
    with tarfile.open(first) as archive:
        members = archive.getmembers()
    assert [m.name for m in members][0] == 'purepoint-pointguard-1.2.3-x86_64-linux-gnu'
    assert all(m.name.startswith('purepoint-pointguard-1.2.3-x86_64-linux-gnu') and '..' not in m.name for m in members)
    assert all((m.isfile() or m.isdir()) and m.uid == 0 and m.gid == 0 and not m.mode & 0o022 for m in members)
    assert {m.name.split('/', 1)[-1]: m.mode for m in members if m.isfile()}['bin/node'] == 0o755
    (tree / 'bin/node').unlink()
    (tree / 'bin/node').symlink_to('/bin/sh')
    problems = module.payload_problems(tree)
    assert any('symlink: bin/node' in p for p in problems) and any('missing: bin/node' in p for p in problems)
    try: module.normalize_tree(tree)
    except ValueError: pass
    else: raise AssertionError('Symlink normalized into a payload')
    (tree / 'bin/node').unlink()
    (tree / 'bin/node').write_bytes(b'n'); (tree / 'bin/node').chmod(0o644)
    assert module.payload_problems(tree) == ['not executable: bin/node']
    (tree / 'bin/node').chmod(0o755)
    (tree / 'bridge/odd\nname').write_bytes(b'x'); (tree / 'bridge/odd\nname').chmod(0o644)
    assert [p.startswith('unsafe name:') for p in module.payload_problems(tree)] == [True]
print('Payload modes, symlink refusal and reproducible tarball PASS')
