import contextlib
import io
from pathlib import Path
import sys
import tempfile
import unittest

from log_macos_command import run_logged


class CommandLogTests(unittest.TestCase):
    def test_combines_raw_output_and_preserves_failure(self):
        with tempfile.TemporaryDirectory() as folder:
            captured = io.TextIOWrapper(io.BytesIO(), encoding="utf8")
            with contextlib.redirect_stdout(captured):
                status = run_logged(
                    [sys.executable, "-c", "import sys; print('started', flush=True); print('diagnostic', file=sys.stderr); sys.exit(7)"], folder,
                )
            self.assertEqual(status, 7)
            raw = (Path(folder) / "xcode.log").read_text()
            self.assertIn("started", raw)
            self.assertIn("diagnostic", raw)

    def test_silent_process_produces_diagnostics_and_still_completes(self):
        with tempfile.TemporaryDirectory() as folder:
            captured = io.TextIOWrapper(io.BytesIO(), encoding="utf8")
            with contextlib.redirect_stdout(captured):
                status = run_logged([sys.executable, "-c", "import time; time.sleep(.3)"], folder, interval=.05)
            self.assertEqual(status, 0)
            self.assertTrue(list(Path(folder).glob("processes-*.txt")))
