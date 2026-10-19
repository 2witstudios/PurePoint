"""Run with python3 -m unittest discover -s crates/pu-engine/verification."""
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from signal_probe_support import source_state_sha256, stop_process


class ProbeSupportTests(unittest.TestCase):
    def test_source_fingerprint_detects_untracked_edits_and_workspace_inputs(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            env = {**os.environ, 'HOME': directory, 'GIT_CONFIG_GLOBAL': os.devnull, 'GIT_CONFIG_SYSTEM': os.devnull}
            def git(*args):
                return subprocess.run(['git', '-C', str(root), *args], env=env, check=True, capture_output=True)
            git('init', '-q')
            (root / 'crates').mkdir()
            tracked = root / 'crates/tracked.rs'
            tracked.write_text('pub fn initial() {}\n')
            (root / '.gitignore').write_text('target/\n')
            git('add', '.')
            git('-c', 'user.name=QA', '-c', 'user.email=qa@example.invalid', '-c', 'commit.gpgsign=false', '-c', 'core.hooksPath=/dev/null', 'commit', '-qm', 'fixture')
            original = source_state_sha256(root)
            new = root / 'crates/untracked.rs'
            new.write_text('first\n')
            first = source_state_sha256(root)
            self.assertNotEqual(original, first)
            new.write_text('second\n')
            self.assertNotEqual(first, source_state_sha256(root))
            new.rename(root / 'crates/renamed.rs')
            renamed = source_state_sha256(root)
            self.assertNotEqual(first, renamed)
            (root / 'crates/renamed.rs').unlink()
            self.assertEqual(original, source_state_sha256(root))
            (root / 'Cargo.lock').write_text('changed workspace dependency\n')
            self.assertNotEqual(original, source_state_sha256(root))
            (root / 'Cargo.lock').unlink()
            tracked.write_text('pub fn updated() {}\n')
            self.assertNotEqual(original, source_state_sha256(root))
            tracked.write_text('pub fn initial() {}\n')
            (root / 'target').mkdir()
            (root / 'target/cache').write_text('irrelevant build cache')
            self.assertEqual(original, source_state_sha256(root))

    def test_cleanup_kills_a_runner_that_ignores_termination(self):
        process = subprocess.Popen([
            sys.executable, '-c',
            'import signal,time; signal.signal(signal.SIGTERM,signal.SIG_IGN); print("ready",flush=True); time.sleep(30)',
        ], stdout=subprocess.PIPE, text=True)
        try:
            self.assertEqual(process.stdout.readline().strip(), 'ready')
            stop_process(process, timeout=0.1)
            self.assertIsNotNone(process.poll())
            stop_process(process, timeout=0.1)
        finally:
            if process.poll() is None:
                process.kill()
                process.wait(timeout=3)
            process.stdout.close()


if __name__ == '__main__':
    unittest.main()
