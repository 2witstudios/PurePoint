"""Offline integrity, reproducibility and relocation proofs for the workflow package."""
import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import shutil
import subprocess
import tarfile
import tempfile
import unittest
from unittest.mock import patch

PACKAGE = Path(__file__).resolve().parents[1] / 'support/pointguard-workflows'


class WorkflowPackageTests(unittest.TestCase):
    def test_runtime_stage_retains_complete_relocatable_package(self):
        spec = importlib.util.spec_from_file_location('workflow_package_runtime',
                                                     Path(__file__).with_name('package-runtime.py'))
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        with tempfile.TemporaryDirectory(prefix='workflow-artifact-') as folder:
            root = Path(folder)
            cache = root / 'cache'
            cache.mkdir()
            payload = io.BytesIO()
            with tarfile.open(fileobj=payload, mode='w:gz') as archive:
                member = tarfile.TarInfo('node-fixture/bin/node')
                member.size = 7
                archive.addfile(member, io.BytesIO(b'fixture'))
            archive = cache / 'node-fixture.tar.gz'
            archive.write_bytes(payload.getvalue())
            pin = {'version': 'fixture', 'archives': {'x64': {
                'name': archive.name, 'sha256': hashlib.sha256(payload.getvalue()).hexdigest()}}}
            pu = root / 'pu'
            helper = root / 'pu-point-guard-lock'
            pu.write_bytes(b'fixture pu')
            helper.write_bytes(b'fixture helper')
            real_run = module.run
            def offline_run(*args, **kwargs):
                if args[1:2] == ('ci',):
                    (Path(kwargs['cwd']) / 'node_modules').mkdir()
                else:
                    real_run(*args, **kwargs)
            # Exercise the real stage/copy/manifest boundary; only executable/dependency
            # downloads are fixtures. This is not a runnable or signed app proof.
            with patch.object(module, 'PIN', pin), patch.object(module, 'run', offline_run), \
                 patch.object(module, 'complete_darwin_graph'):
                target = module.stage(root / 'Build.app', pu, 'x64', cache, lock_helper=helper)
            relocated = root / 'relocated artifact.app'
            shutil.move(str(root / 'Build.app'), relocated)
            resources = relocated / 'Contents/Resources/PointGuard'
            manifest = json.loads((resources / 'runtime-manifest.json').read_text())
            package = resources / manifest['paths']['workflowPackage']
            subprocess.run(['python3', str(package / 'tools/import-workflows.py'), '--verify'], check=True)
            for original in PACKAGE.rglob('*'):
                if original.is_file():
                    self.assertEqual((package / original.relative_to(PACKAGE)).read_bytes(), original.read_bytes())

    @unittest.skipUnless(os.environ.get('POINTGUARD_TEST_SOURCE'), 'Set explicit POINTGUARD_TEST_SOURCE for git provenance proof')
    def test_reproducible_import_matches_every_pinned_git_blob(self):
        source = os.environ['POINTGUARD_TEST_SOURCE']
        initial_status = subprocess.check_output(['git', '-C', source, 'status', '--porcelain'])
        with tempfile.TemporaryDirectory(prefix='workflow-regenerate-') as folder:
            regenerated = Path(folder) / 'regenerated'
            subprocess.run(['python3', str(PACKAGE / 'tools/import-workflows.py'),
                            '--source', source, '--output', str(regenerated)], check=True)
            proof = json.loads((regenerated / 'source-manifest.json').read_text())
            tree = subprocess.check_output(['git', '-C', source, 'ls-tree', '-rz', proof['commit']])
            entries = [entry for entry in tree.split(b'\0') if entry]
            self.assertEqual(len(entries), len(proof['files']))
            for entry in entries:
                header, name = entry.split(b'\t', 1)
                mode, kind, blob = header.decode().split()
                self.assertEqual(kind, 'blob')
                name = name.decode()
                original = subprocess.check_output(['git', '-C', source, 'cat-file', 'blob', blob])
                self.assertEqual((regenerated / 'source' / name).read_bytes(), original, name)
                self.assertEqual((PACKAGE / 'source' / name).read_bytes(), original, name)
            for path in regenerated.rglob('*'):
                if path.is_file():
                    relative = path.relative_to(regenerated)
                    self.assertEqual(path.read_bytes(), (PACKAGE / relative).read_bytes(), str(relative))
        self.assertEqual(subprocess.check_output(['git', '-C', source, 'status', '--porcelain']), initial_status)

    def test_missing_pin_refuses_to_replace_existing_package(self):
        with tempfile.TemporaryDirectory(prefix='workflow-invalid-input-') as folder:
            root = Path(folder)
            source = root / 'wrong-source'
            source.mkdir()
            subprocess.run(['git', 'init', '--quiet', str(source)], check=True)
            output = root / 'existing-package'
            (output / 'source').mkdir(parents=True)
            sentinel = output / 'source/preserve.txt'
            sentinel.write_text('existing package')
            result = subprocess.run(['python3', str(PACKAGE / 'tools/import-workflows.py'),
                                     '--source', str(source), '--output', str(output)],
                                    capture_output=True, text=True)
            self.assertNotEqual(result.returncode, 0)
            self.assertEqual(sentinel.read_text(), 'existing package')

    def test_stray_adapter_files_and_symlinks_fail_verification(self):
        with tempfile.TemporaryDirectory(prefix='workflow-stray-') as folder:
            relocated = Path(folder) / 'workflows'
            shutil.copytree(PACKAGE, relocated)
            for kind in ('reference', 'nested', 'symlink'):
                with self.subTest(kind=kind):
                    stray = relocated / 'skills' / ('stray-' + kind)
                    if kind == 'symlink':
                        stray.symlink_to('missing-target')
                    else:
                        stray.mkdir()
                        (stray / 'reference.md').write_text('unexpected shipped file')
                    result = subprocess.run(['python3', str(relocated / 'tools/import-workflows.py'),
                                             '--verify'], capture_output=True, text=True)
                    self.assertNotEqual(result.returncode, 0, kind)
                    if stray.is_symlink():
                        stray.unlink()
                    else:
                        shutil.rmtree(stray)

    def test_complete_pinned_source_and_relocation(self):
        with tempfile.TemporaryDirectory(prefix='workflow-relocated-') as folder:
            relocated = Path(folder) / 'resources with spaces/workflows'
            shutil.copytree(PACKAGE, relocated)
            proof = json.loads((relocated / 'source-manifest.json').read_text())
            self.assertEqual(proof['commit'], 'a12cecee28e8c22c250da6c30aecbdcad2f38fe5')
            self.assertEqual(len(proof['files']), 26)
            self.assertEqual(set(proof['files']), {
                p.relative_to(relocated / 'source').as_posix()
                for p in (relocated / 'source').rglob('*') if p.is_file()
            })
            for name, entry in proof['files'].items():
                path = relocated / 'source' / name
                self.assertEqual(hashlib.sha256(path.read_bytes()).hexdigest(), entry['sha256'], name)
                self.assertEqual(path.stat().st_mode & 0o777, int(entry['mode'], 8), name)
            # Real upstream relative resources: preserved at their original paths.
            for name in ['skills/geo-interview/fan-out-queries.md',
                         'skills/geo-interview/geo-scoring.md',
                         'skills/pagespace-cli/references/tool-mapping.md',
                         'skills/review/references/review-example.md',
                         'LICENSE', 'NOTICE.md', 'install.sh']:
                self.assertTrue((relocated / 'source' / name).is_file(), name)
            result = subprocess.run(['python3', str(relocated / 'tools/import-workflows.py'),
                                     '--verify'], text=True, capture_output=True)
            self.assertEqual(result.returncode, 0, result.stderr)
            (relocated / 'source/skills/task/SKILL.md').write_text('tampered')
            result = subprocess.run(['python3', str(relocated / 'tools/import-workflows.py'),
                                     '--verify'], text=True, capture_output=True)
            self.assertNotEqual(result.returncode, 0, 'Tampered source must fail verification')


if __name__ == '__main__':
    unittest.main()
