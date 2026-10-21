"""Release pipeline invariants: who holds which credential, and what code runs next to it."""

import json
import re
import unittest
from pathlib import Path

import yaml

ROOT = Path(__file__).resolve().parents[2]
WORKFLOWS = sorted((ROOT / ".github/workflows").glob("*.yml"))
RELEASE = ROOT / ".github/workflows/release.yml"
PACKAGE_RESOLVED = ROOT / "apps/purepoint-macos/purepoint-macos.xcodeproj/project.xcworkspace/xcshareddata/swiftpm/Package.resolved"


class UniqueKeyLoader(yaml.SafeLoader):
    """GitHub rejects a workflow that repeats a key; PyYAML silently keeps the last one."""

    def construct_mapping(self, node, deep=False):
        keys = [self.construct_object(key, deep=deep) for key, _ in node.value]
        duplicates = {key for key in keys if keys.count(key) > 1}
        if duplicates:
            raise yaml.constructor.ConstructorError(None, None, f"duplicate keys {duplicates}", node.start_mark)
        return super().construct_mapping(node, deep=deep)


def load(path):
    return yaml.load(path.read_text(), Loader=UniqueKeyLoader)


def steps(job):
    return job.get("steps", [])


def run_text(job):
    return "\n".join(step.get("run", "") for step in steps(job))


class PinningTests(unittest.TestCase):
    def test_workflows_repeat_no_keys(self):
        for path in WORKFLOWS:
            with self.subTest(file=path.name):
                load(path)

    def test_every_action_is_pinned_to_a_commit_sha_with_a_version_comment(self):
        uses = re.compile(r"^\s*(?:-\s+)?uses:\s*(\S+)(.*)$")
        for path in WORKFLOWS:
            for number, line in enumerate(path.read_text().splitlines(), 1):
                match = uses.match(line)
                if not match or match.group(1).startswith("./"):
                    continue
                with self.subTest(file=path.name, line=number):
                    self.assertRegex(match.group(1), r"^[\w.-]+/[\w./-]+@[0-9a-f]{40}$")
                    self.assertRegex(match.group(2), r"^\s+#\s+\S+")

    def test_every_checkout_leaves_no_token_in_git_config(self):
        for path in WORKFLOWS:
            for name, job in load(path)["jobs"].items():
                for step in steps(job):
                    if step.get("uses", "").startswith("actions/checkout@"):
                        with self.subTest(file=path.name, job=name):
                            self.assertIs((step.get("with") or {}).get("persist-credentials"), False)


    def test_every_workflow_defaults_to_a_read_only_token(self):
        for path in WORKFLOWS:
            with self.subTest(file=path.name):
                self.assertIn(load(path).get("permissions"), ({}, {"contents": "read"}))


class ReleaseWorkflowTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.workflow = load(RELEASE)
        cls.jobs = cls.workflow["jobs"]
        cls.text = RELEASE.read_text()

    def test_runs_only_for_version_tag_pushes(self):
        # PyYAML reads the bare `on` key as boolean True.
        triggers = self.workflow[True]
        self.assertEqual(list(triggers), ["push"])
        self.assertEqual(triggers["push"], {"tags": ["v*"]})

    def test_token_permissions_are_granted_per_job(self):
        self.assertEqual(self.workflow["permissions"], {})
        writers = []
        for name, job in self.jobs.items():
            with self.subTest(job=name):
                self.assertIn("permissions", job)
                if "write" in job["permissions"].values():
                    writers.append(name)
        self.assertEqual(writers, ["publish"])
        self.assertEqual(self.jobs["publish"]["permissions"], {"contents": "write"})

    def test_write_token_reaches_only_the_steps_that_publish(self):
        publish = self.jobs["publish"]
        self.assertNotIn("GH_TOKEN", publish.get("env", {}))
        holders = [step["name"] for step in steps(publish) if "GH_TOKEN" in (step.get("env") or {})]
        self.assertEqual(holders, ["Create GitHub Release", "Generate and upload appcast"])

    def test_existing_release_must_match_the_new_assets_byte_for_byte(self):
        # Re-signing changes the DMG, so a name-only check could publish an
        # appcast signature for bytes that were never released.
        create = next(step["run"] for step in steps(self.jobs["publish"]) if step["name"] == "Create GitHub Release")
        self.assertIn(".digest", create)
        self.assertIn("sha256sum", create)
        self.assertIn("grep -qxF \"$EXPECTED\"", create)
        self.assertNotIn("--clobber", create)

    def test_artifacts_outlive_an_environment_approval_wait(self):
        for name in ("build-macos", "sign-macos"):
            for step in steps(self.jobs[name]):
                if step.get("uses", "").startswith("actions/upload-artifact@"):
                    with self.subTest(job=name):
                        self.assertGreaterEqual(step["with"]["retention-days"], 14)

    def test_create_dmg_is_fetched_by_commit(self):
        env = self.jobs["sign-macos"]["env"]
        self.assertRegex(env["CREATE_DMG_COMMIT"], r"^[0-9a-f]{40}$")
        self.assertRegex(env["CREATE_DMG_SHA256"], r"^[0-9a-f]{64}$")
        self.assertNotIn("refs/tags", run_text(self.jobs["sign-macos"]))

    def test_publish_job_runs_no_repository_code(self):
        for step in steps(self.jobs["publish"]):
            self.assertFalse(step.get("uses", "").startswith("actions/checkout@"))

    def test_secrets_reach_only_the_protected_signing_job(self):
        sign = self.jobs["sign-macos"]
        self.assertEqual(sign["environment"], "release")
        self.assertIn("refs/tags/v", sign["if"])
        for name, job in self.jobs.items():
            if name != "sign-macos":
                with self.subTest(job=name):
                    self.assertNotIn("secrets.", json.dumps(job))

    def test_build_job_produces_an_unsigned_app(self):
        archive = run_text(self.jobs["build-macos"])
        self.assertIn("CODE_SIGN_IDENTITY=-", archive)
        self.assertIn("CODE_SIGNING_REQUIRED=NO", archive)

    def test_release_builds_restore_no_cache(self):
        for name, job in self.jobs.items():
            for step in steps(job):
                with self.subTest(job=name, step=step.get("name") or step.get("uses")):
                    self.assertNotRegex(step.get("uses", ""), r"(rust-cache|actions/cache)@")
                    self.assertNotIn("cache", step.get("with") or {})

    def test_signing_identity_is_restricted_and_removed(self):
        sign = run_text(self.jobs["sign-macos"])
        imports = [line for line in sign.splitlines() if "security import" in line or line.strip().startswith("-x ")]
        self.assertTrue(imports)
        joined = " ".join(imports)
        self.assertIn("-T /usr/bin/codesign", joined)
        self.assertIn(" -x ", f" {joined} ")
        self.assertNotRegex(joined, r"(^|\s)-A(\s|$)")
        self.assertRegex(sign, r"set-keychain-settings -lut (\d{1,3}|1[0-7]\d\d) ")
        cleanup = steps(self.jobs["sign-macos"])[-1]
        self.assertEqual(cleanup["if"], "always()")
        self.assertIn('security delete-keychain "$KEYCHAIN_PATH"', cleanup["run"])
        self.assertIn('rm -f "$CERT_PATH"', cleanup["run"])

    def test_sparkle_tools_match_the_linked_framework_and_are_verified(self):
        resolved = json.loads(PACKAGE_RESOLVED.read_text())
        linked = next(pin["state"]["version"] for pin in resolved["pins"] if pin["identity"] == "sparkle")
        env = self.jobs["sign-macos"]["env"]
        self.assertEqual(str(env["SPARKLE_VERSION"]), linked)
        self.assertRegex(env["SPARKLE_SHA256"], r"^[0-9a-f]{64}$")
        install = next(step["run"] for step in steps(self.jobs["sign-macos"]) if step.get("name") == "Install Sparkle tools")
        self.assertIn("curl -fsSL", install)
        self.assertIn("shasum -a 256 -c", install)

    def test_downloaded_tools_are_checksum_verified(self):
        for line in run_text(self.jobs["sign-macos"]).splitlines():
            if "curl " in line:
                self.assertIn("-fsSL", line)
        self.assertEqual(self.text.count("curl -fsSL"), self.text.count("shasum -a 256 -c"))
        self.assertNotIn("brew install", run_text(self.jobs["sign-macos"]))

    def test_update_key_output_is_not_logged(self):
        sign = run_text(self.jobs["sign-macos"])
        for line in sign.splitlines():
            if "SPARKLE_ED25519_KEY" in line or "SIGN_OUTPUT" in line:
                with self.subTest(line=line.strip()):
                    self.assertRegex(line, r"\|\s*(sed|\"\$RUNNER_TEMP/sparkle/bin/sign_update\")")

    def test_only_the_update_feed_is_ever_replaced(self):
        for line in self.text.splitlines():
            if "--clobber" in line:
                with self.subTest(line=line.strip()):
                    self.assertEqual(line.strip(), "gh release upload appcast build/appcast.xml --clobber")


if __name__ == "__main__":
    unittest.main()
