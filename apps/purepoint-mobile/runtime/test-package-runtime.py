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
