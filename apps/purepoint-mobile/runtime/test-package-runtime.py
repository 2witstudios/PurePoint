"""Disposable offline regression for interrupted native-package downloads."""
import base64
import hashlib
import importlib.util
import io
import json
from pathlib import Path
import tarfile
import tempfile
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
    with patch.object(module, 'SOURCE', source), patch.object(module.urllib.request, 'urlretrieve', interrupted):
        try: module.complete_darwin_graph(modules, ['x64'], cache)
        except OSError: pass
        else: raise AssertionError('Interrupted download accepted')
    assert list(cache.iterdir()) == [], 'Partial download must never become a cache entry'
    with patch.object(module, 'SOURCE', source), patch.object(module.urllib.request, 'urlretrieve', valid):
        module.complete_darwin_graph(modules, ['x64'], cache)
    assert (modules / 'fixture/bin/tool').read_bytes() == b'valid'
    assert len(list(cache.iterdir())) == 1
print('Interrupted native download cleanup and exact-integrity retry PASS')
