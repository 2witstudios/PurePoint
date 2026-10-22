import json
import os
import re
import subprocess
import tempfile
import unittest
from unittest.mock import patch

from pathlib import Path

from ci import CHECKS, changed_paths, components, detect, require_checks

WORKFLOW = Path(__file__).resolve().parents[1] / "workflows" / "macos.yml"


class RoutingTests(unittest.TestCase):
    def test_docs_mobile_macos_rust_and_workflow_changes(self):
        routed = ("macos", "mobile", "mobile_swift", "packaging")
        for paths, expected in (
            (["README.md"], ()),
            (["apps/shared/ChatMarkdown/ChatMarkdown.swift"], routed),
            (["apps/purepoint-mobile/bridge/core.js"], ("macos", "mobile", "mobile_swift", "packaging")),
            (["apps/purepoint-mobile/bridge/core.test.js"], ("macos", "mobile", "mobile_swift")),
            (["apps/purepoint-mobile/runtime/package-runtime.py"], ("macos", "mobile", "packaging")),
            (["apps/purepoint-mobile/runtime/verify-simulator-signing.py"], ("macos", "mobile", "mobile_swift", "packaging")),
            (["apps/purepoint-mobile/package-lock.json"], routed),
            (["apps/purepoint-mobile/package.json"], routed),
            (["apps/purepoint-mobile/docs/point-guard.md"], ("macos", "mobile", "packaging")),
            (["apps/purepoint-mobile/support/pu/SKILL.md"], ("macos", "mobile", "packaging")),
            (["apps/purepoint-mobile/PurePoint/ChatModel.swift"], ("mobile", "mobile_swift")),
            (["apps/purepoint-mobile/PurePointTests/ChatTests.swift"], ("mobile", "mobile_swift")),
            (["apps/purepoint-mobile/verification/check-trust.py"], ("mobile", "mobile_swift")),
            (["apps/purepoint-mobile/PurePoint.xcodeproj/project.pbxproj"], ("mobile", "mobile_swift", "packaging")),
            (["apps/purepoint-mobile/README.md"], ("mobile",)),
            (["apps/purepoint-macos/App.swift"], ("macos",)),
            (["apps/purepoint-macos/purepoint-macos.xcodeproj/project.pbxproj"], ("macos", "packaging")),
            (["apps/purepoint-macos/purepoint-macos.xcodeproj/project.xcworkspace/xcshareddata/swiftpm/Package.resolved"], ("macos", "packaging")),
            (["crates/pu-core/src/lib.rs"], ("macos", "mobile")),
            (["crates/pu-cli/src/bin/point_guard_lock.rs"], routed),
            (["crates/pu-cli/Cargo.toml"], routed),
            (["Cargo.lock"], ("macos", "mobile", "mobile_swift")),
            ([".github/workflows/release-proof.yml"], routed),
            ([".github/workflows/release.yml"], routed),
            ([".github/workflows/macos.yml"], routed),
            ([".github/scripts/ci.py"], routed),
            (["apps/purepoint-mobile/a", "crates/pu-core/src/lib.rs"], ("macos", "mobile")),
        ):
            with self.subTest(paths=paths):
                flags = components(paths)
                self.assertEqual({name for name in routed if flags[name]}, set(expected))
                self.assertEqual(
                    flags["release_archive"],
                    flags["packaging"] or any(path.startswith("apps/purepoint-macos/") for path in paths),
                )
                # Pull requests never run the Intel legs or the universal proof.
                self.assertFalse(flags["universal"])
                self.assertEqual([t["runner"] for t in flags["runtime_targets"]], ["macos-26"])

    def test_linux_phone_payload_routing(self):
        for paths, expected in (
            (["README.md"], False),
            (["apps/purepoint-mobile/PurePoint/ChatModel.swift"], False),
            (["apps/purepoint-mobile/bridge/core.test.js"], False),
            (["crates/pu-engine/src/engine.rs"], False),
            (["apps/purepoint-mobile/bridge/core.js"], True),
            (["apps/purepoint-mobile/runtime/package-runtime.py"], True),
            (["apps/purepoint-mobile/runtime/prove-linux-runtime.mjs"], True),
            (["apps/purepoint-mobile/runtime/node-release.json"], True),
            (["apps/purepoint-mobile/package-lock.json"], True),
            (["scripts/install.sh"], True),
            (["crates/pu-cli/src/phone.rs"], True),
            (["crates/pu-cli/src/commands/phone.rs"], True),
            (["Cargo.lock"], True),
            ([".github/workflows/release.yml"], True),
            ([".github/workflows/macos.yml"], True),
        ):
            with self.subTest(paths=paths):
                self.assertIs(components(paths)["linux_phone"], expected)
        self.assertTrue(components([], full=True)["linux_phone"])

    def test_crates_only_pull_request_skips_packaging_and_ios(self):
        flags = components(["crates/pu-engine/src/engine.rs", "crates/pu-core/src/protocol.rs"])
        self.assertFalse(flags["packaging"])
        self.assertFalse(flags["release_archive"])
        self.assertFalse(flags["mobile_swift"])
        self.assertFalse(flags["universal"])

    def test_full_runs_prove_everything_including_intel(self):
        flags = components([], full=True)
        self.assertTrue(all(flags[name] for name in ("macos", "mobile", "mobile_swift", "packaging", "release_archive", "universal")))
        self.assertEqual([t["architecture"] for t in flags["runtime_targets"]], ["arm64", "x64"])

    def test_only_pull_requests_are_routed_by_path(self):
        with tempfile.TemporaryDirectory() as folder:
            event_path = os.path.join(folder, "event.json")
            with open(event_path, "w") as file:
                json.dump({"pull_request": {"base": {"sha": "a"}, "head": {"sha": "b"}}}, file)
            with patch("ci.changed_paths", return_value=["crates/pu-core/src/lib.rs"]):
                for event_name, full in (("pull_request", False), ("push", True), ("schedule", True), ("workflow_dispatch", True)):
                    with self.subTest(event_name=event_name):
                        outputs = detect(event_name, event_path)
                        for name in ("macos", "mobile", "mobile_swift", "packaging", "release_archive", "universal"):
                            self.assertIn(outputs[name], ("true", "false"))
                        self.assertEqual(outputs["packaging"], str(full).lower())
                        self.assertEqual(outputs["universal"], str(full).lower())
                        self.assertEqual(len(json.loads(outputs["runtime_targets"])), 2 if full else 1)

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


class WorkflowTests(unittest.TestCase):
    """Keep macos.yml's job conditions and gate needs in step with CHECKS."""

    def jobs(self):
        # Line-based so the test needs no YAML library on the runner.
        jobs, name = {}, None
        for line in WORKFLOW.read_text().split("\n")[WORKFLOW.read_text().split("\n").index("jobs:") + 1:]:
            if re.fullmatch(r"  [a-z-]+:", line):
                name = line.strip()[:-1]
                jobs[name] = {}
            elif name and (match := re.fullmatch(r"    (needs|if): (.+)", line)):
                jobs[name][match[1]] = match[2]
        return jobs

    def test_gate_needs_every_routed_job(self):
        needs = self.jobs()["build-and-test"]["needs"]
        self.assertEqual(set(needs.strip("[]").split(", ")), {"changes"} | {job for job, _ in CHECKS})

    def test_each_routed_job_runs_on_its_component(self):
        jobs = self.jobs()
        for job, component in CHECKS:
            with self.subTest(job=job):
                self.assertEqual(jobs[job]["if"], f"needs.changes.outputs.{component} == 'true'")
                self.assertIn(f"{component}: ${{{{ steps.paths.outputs.{component} }}}}", WORKFLOW.read_text())


class GateTests(unittest.TestCase):
    COMPONENTS = sorted({component for _, component in CHECKS})

    def results(self, *required):
        return {
            "changes": {"result": "success", "outputs": {c: str(c in required).lower() for c in self.COMPONENTS}},
            **{job: {"result": "success" if component in required else "skipped"} for job, component in CHECKS},
        }

    def test_every_routed_job_is_gated(self):
        self.assertEqual({job for job, _ in CHECKS}, {
            "macos", "release-archive", "mobile-bridge", "mobile-swift",
            "runtime-package", "runtime-universal", "runtime-rust", "runtime-linux",
        })

    def test_only_unaffected_jobs_may_skip(self):
        require_checks(self.results())
        require_checks(self.results(*self.COMPONENTS))
        for job, component in CHECKS:
            require_checks(self.results(component))
            for result in ("failure", "cancelled", "skipped"):
                with self.subTest(job=job, result=result):
                    results = self.results(*self.COMPONENTS)
                    results[job]["result"] = result
                    with self.assertRaises(ValueError):
                        require_checks(results)

    def test_unrequired_job_cannot_run_unexpectedly(self):
        results = self.results("mobile")
        results["runtime-package"]["result"] = "success"
        with self.assertRaises(ValueError):
            require_checks(results)

    def test_release_failure_cannot_hide_behind_debug_success(self):
        for result in ("failure", "cancelled", "skipped"):
            results = self.results("macos", "release_archive")
            results["release-archive"]["result"] = result
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
