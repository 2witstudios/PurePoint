import os
import subprocess
import tempfile
import unittest
from unittest.mock import patch

from ci import changed_paths, components, require_checks


class RoutingTests(unittest.TestCase):
    def test_docs_mobile_macos_rust_and_workflow_changes(self):
        for paths, expected in (
            (["README.md"], {"macos": False, "mobile": False}),
            (["apps/purepoint-mobile/bridge/core.js"], {"macos": False, "mobile": True}),
            (["apps/purepoint-macos/App.swift"], {"macos": True, "mobile": False}),
            (["crates/pu-core/src/lib.rs"], {"macos": True, "mobile": False}),
            ([".github/workflows/macos.yml"], {"macos": True, "mobile": True}),
            ([".github/scripts/ci.py"], {"macos": True, "mobile": True}),
            (["apps/purepoint-mobile/a", "Cargo.lock"], {"macos": True, "mobile": True}),
        ):
            with self.subTest(paths=paths):
                self.assertEqual(components(paths), expected)

    def test_diff_includes_deleted_side_of_cross_component_rename(self):
        with tempfile.TemporaryDirectory() as folder:
            def git(*args):
                return subprocess.check_output(["git", "-C", folder, *args]).decode().strip()
            git("init", "-q")
            git("config", "user.name", "CI Test")
            git("config", "user.email", "ci@example.invalid")
            original = os.path.join(folder, "apps", "purepoint-mobile", "source.txt")
            os.makedirs(os.path.dirname(original))
            with open(original, "w") as file:
                file.write("move between components")
            git("add", ".")
            git("commit", "-qm", "base")
            base = git("rev-parse", "HEAD")
            git("mv", "apps/purepoint-mobile/source.txt", "README.md")
            git("commit", "-qm", "rename")
            head = git("rev-parse", "HEAD")
            original_run = subprocess.check_output
            with patch("ci.subprocess.check_output", side_effect=lambda args: original_run(args, cwd=folder)):
                for event in (
                    {"pull_request": {"base": {"sha": base}, "head": {"sha": head}}},
                    {"before": base, "after": head},
                    {"before": "0" * 40, "after": base},
                ):
                    self.assertTrue(components(changed_paths(event))["mobile"])


class GateTests(unittest.TestCase):
    def results(self, macos=False, mobile=False):
        return {
            "changes": {"result": "success", "outputs": {"macos": str(macos).lower(), "mobile": str(mobile).lower()}},
            "macos": {"result": "success" if macos else "skipped"},
            "mobile-bridge": {"result": "success" if mobile else "skipped"},
            "mobile-swift": {"result": "success" if mobile else "skipped"},
        }

    def test_only_unaffected_jobs_may_skip(self):
        for macos, mobile in ((False, False), (True, False), (False, True), (True, True)):
            require_checks(self.results(macos, mobile))
        for result in ("failure", "cancelled", "skipped"):
            results = self.results(mobile=True)
            results["mobile-swift"]["result"] = result
            with self.assertRaises(ValueError):
                require_checks(results)

    def test_detection_failure_or_missing_output_cannot_pass(self):
        for result in ("failure", "cancelled", "skipped"):
            results = self.results()
            results["changes"]["result"] = result
            with self.assertRaises(ValueError):
                require_checks(results)
        results = self.results()
        del results["changes"]["outputs"]["mobile"]
        with self.assertRaises(ValueError):
            require_checks(results)


if __name__ == "__main__":
    unittest.main()
