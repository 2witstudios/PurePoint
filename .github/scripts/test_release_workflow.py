"""Release pipeline invariants: who holds which credential, and what code runs next to it."""

import json
import re
import unittest
from pathlib import Path

import yaml

ROOT = Path(__file__).resolve().parents[2]
WORKFLOWS = sorted((ROOT / ".github/workflows").glob("*.yml"))
RELEASE = ROOT / ".github/workflows/release.yml"
RELEASE_PROOF = ROOT / ".github/workflows/release-proof.yml"
MACOS = ROOT / ".github/workflows/macos.yml"
INSTALL = ROOT / "scripts/install.sh"
INSTALL_PS1 = ROOT / "scripts/install.ps1"
INFO_PLIST = ROOT / "apps/purepoint-macos/Info.plist"
PBXPROJ = ROOT / "apps/purepoint-macos/purepoint-macos.xcodeproj/project.pbxproj"
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


    def test_no_workflow_installs_unpinned_homebrew_tools(self):
        for path in WORKFLOWS:
            with self.subTest(file=path.name):
                self.assertNotIn("brew install", path.read_text())

    def test_app_builds_resolve_swift_packages_only_from_the_committed_file(self):
        for path in (RELEASE, RELEASE_PROOF, MACOS):
            commands = re.findall(r"xcodebuild (?:archive|build-for-testing)\b.*?(?=\n\s*\n|\n\s*-\s+name:|\Z)", path.read_text(), re.S)
            self.assertTrue(commands, path.name)
            for command in commands:
                with self.subTest(file=path.name, command=command.splitlines()[0]):
                    self.assertIn("-onlyUsePackageVersionsFromResolvedFile", command)

    def test_swift_packages_are_never_tracked_by_branch(self):
        self.assertNotIn("kind = branch", PBXPROJ.read_text())
        resolved = json.loads(PACKAGE_RESOLVED.read_text())
        direct = set(re.findall(r'repositoryURL = "([^"]+)"', PBXPROJ.read_text()))
        for pin in resolved["pins"]:
            if pin["location"] in direct:
                with self.subTest(package=pin["identity"]):
                    self.assertNotIn("branch", pin["state"])

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
        self.assertEqual(writers, ["attest", "publish"])
        self.assertEqual(self.jobs["publish"]["permissions"], {"contents": "write"})
        self.assertEqual(self.jobs["attest"]["permissions"], {
            "contents": "read", "id-token": "write", "attestations": "write", "artifact-metadata": "write",
        })


    def test_phone_payloads_are_signed_attested_and_gate_publishing(self):
        self.assertIn("linux-phone", self.jobs["sign-linux"]["needs"])
        self.assertIn("linux-phone", self.jobs["attest"]["needs"])
        self.assertIn("linux-phone", self.jobs["publish"]["needs"])
        npm_sbom = [step for step in steps(self.jobs["attest"]) if step.get("name") == "Attest Point Guard npm SBOM"]
        self.assertIn("dist/purepoint-pointguard-*.tar.gz", npm_sbom[0]["with"]["subject-path"])

    def test_linux_build_ships_the_lock_helper_and_phone_payload_is_proven(self):
        self.assertIn("pu-point-guard-lock", run_text(self.jobs["linux"]))
        phone = self.jobs["linux-phone"]
        self.assertEqual(phone["permissions"], {"contents": "read"})
        self.assertEqual(set(phone["needs"]), {"version", "linux"})
        self.assertEqual(
            {(leg["arch"], leg["node_target"], leg["runner"]) for leg in phone["strategy"]["matrix"]["include"]},
            {("x86_64", "linux-x64", "ubuntu-latest"), ("aarch64", "linux-arm64", "ubuntu-24.04-arm")},
        )
        text = run_text(phone)
        self.assertIn("--layout dir", text)
        self.assertIn('--lock-helper "$PU_DIR/pu-point-guard-lock"', text)
        self.assertIn("sha256sum -c", text)
        self.assertIn("prove-linux-runtime.mjs", text)
        self.assertIn("env -i HOME=", text)
        self.assertIn("--in-place", text)
        # No ${{ }} expression is expanded inside a run script.
        for step in steps(phone):
            self.assertNotIn("${{", step.get("run", ""))
    def test_write_token_reaches_only_the_steps_that_publish(self):
        publish = self.jobs["publish"]
        self.assertNotIn("GH_TOKEN", publish.get("env", {}))
        holders = [step["name"] for step in steps(publish) if "GH_TOKEN" in (step.get("env") or {})]
        self.assertEqual(holders, ["Create GitHub Release", "Upload appcast"])

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

    def test_jobs_with_write_scopes_run_no_repository_code(self):
        for name in ("attest", "publish"):
            for step in steps(self.jobs[name]):
                with self.subTest(job=name, step=step.get("name")):
                    self.assertFalse(step.get("uses", "").startswith("actions/checkout@"))
                    self.assertNotIn("run", step if name == "attest" else {})

    def test_secrets_reach_only_the_protected_signing_jobs(self):
        signers = {"sign-macos", "sign-linux", "sign-windows"}
        for name in signers:
            with self.subTest(job=name):
                self.assertEqual(self.jobs[name]["environment"], "release")
                self.assertIn("refs/tags/v", self.jobs[name]["if"])
        for name, job in self.jobs.items():
            if name not in signers:
                with self.subTest(job=name):
                    self.assertNotIn("secrets.", json.dumps(job))

    def test_nothing_is_published_before_it_is_signed_and_attested(self):
        self.assertTrue({"sign-macos", "sign-linux", "sign-windows", "attest", "sbom"} <= set(self.jobs["publish"]["needs"]))
        self.assertTrue({"sign-macos", "linux", "sign-windows", "sbom"} <= set(self.jobs["attest"]["needs"]))

    def test_every_release_binary_has_provenance_and_sbom_attestations(self):
        attests = [step for step in steps(self.jobs["attest"]) if step.get("uses", "").startswith("actions/attest@")]
        provenance = [step["with"] for step in attests if "sbom-path" not in step["with"]]
        self.assertEqual(len(provenance), 1)
        self.assertIn("dist/*.tar.gz", provenance[0]["subject-path"])
        self.assertIn("dist/*.dmg", provenance[0]["subject-path"])
        sboms = {re.search(r"\}-(.+)\.cdx\.json$", step["with"]["sbom-path"]).group(1): step["with"]["subject-path"]
                 for step in attests if "sbom-path" in step["with"]}
        self.assertEqual(set(sboms), {"pu-cli", "pu-engine", "point-guard-npm"})
        for name in ("pu-cli", "pu-engine"):
            self.assertIn("dist/*.tar.gz", sboms[name])
            self.assertIn("dist/*.dmg", sboms[name])
        self.assertIn("dist/*.dmg", sboms["point-guard-npm"])

    def test_sboms_cover_the_rust_binaries_and_the_npm_runtime(self):
        sbom = run_text(self.jobs["sbom"])
        self.assertIn("cargo cyclonedx --format json", sbom)
        self.assertIn("--target all", sbom)
        self.assertIn("for crate in pu-cli pu-engine", sbom)
        self.assertIn("npm sbom --omit=dev --sbom-format cyclonedx", sbom)
        self.assertRegex(self.jobs["sbom"]["env"]["CARGO_CYCLONEDX_VERSION"], r"^\d+\.\d+\.\d+$")
        self.assertIn('--locked cargo-cyclonedx --version "$CARGO_CYCLONEDX_VERSION"', sbom)

    def test_linux_archives_are_signed_with_the_key_install_sh_embeds(self):
        sign = next(step for step in steps(self.jobs["sign-linux"]) if step.get("name") == "Sign archives")
        self.assertEqual(sign["env"], {"RELEASE_MINISIGN_KEY": "${{ secrets.RELEASE_MINISIGN_KEY }}"})
        script = sign["run"]
        # Without the secret the release still ships, with a visible warning.
        self.assertIn('if [ -z "$RELEASE_MINISIGN_KEY" ]', script)
        self.assertIn("::warning", script)
        self.assertIn("RELEASE_PUBLIC_KEY", script)
        self.assertIn("scripts/install.sh", script)
        self.assertIn(".github/scripts/minisign.sh sign", script)
        self.assertGreaterEqual(script.count('rm -f "$KEY"'), 2)
        self.assertIn("umask 077", script)
        self.assertRegex(INSTALL.read_text(), r'(?m)^RELEASE_PUBLIC_KEY="RWS[A-Za-z0-9+/]{53}"$')

    def test_appcast_is_signed_by_the_update_key_holder(self):
        sign = run_text(self.jobs["sign-macos"])
        self.assertIn('sign_update" build/appcast/appcast.xml --ed-key-file -', sign)
        self.assertIn('sign_update" --verify build/appcast/appcast.xml --ed-key-file -', sign)
        publish = run_text(self.jobs["publish"])
        self.assertNotIn("<rss", publish)
        plist = INFO_PLIST.read_text()
        for key in ("SURequireSignedFeed", "SUVerifyUpdateBeforeExtraction"):
            with self.subTest(key=key):
                self.assertRegex(plist, rf"<key>{key}</key>\s*<true/>")

    def test_dmg_creation_failures_are_not_ignored(self):
        create = next(step["run"] for step in steps(self.jobs["sign-macos"]) if step.get("name") == "Create DMG")
        self.assertNotIn("|| true", create)
        self.assertIn("|| STATUS=$?", create)
        self.assertIn('hdiutil verify "$DMG_PATH"', create)
        self.assertIn("exit 1", create)

    def test_packaging_uses_the_same_node_as_ci(self):
        node = [step["with"] for step in steps(self.jobs["build-macos"]) if step.get("uses", "").startswith("actions/setup-node@")]
        self.assertEqual(len(node), 1)
        self.assertEqual(str(node[0]["node-version"]), "22.23.3")
        self.assertIs(node[0]["package-manager-cache"], False)

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
                    self.assertIn(line.strip(), {
                        "gh release upload appcast build/appcast.xml --clobber",
                        "gh release upload appcast build/latest-windows.json --clobber",
                    })

    # --- Windows (DIST-009, WIN-10.4) ---

    def test_windows_builds_both_architectures_without_secrets(self):
        build = self.jobs["windows"]
        self.assertEqual(build["permissions"], {"contents": "read"})
        self.assertEqual(build["needs"], "version")
        self.assertEqual(build["runs-on"], "windows-latest")
        self.assertEqual(
            {(leg["arch"], leg["target"]) for leg in build["strategy"]["matrix"]["include"]},
            {("x64", "x86_64-pc-windows-msvc"), ("arm64", "aarch64-pc-windows-msvc")},
        )
        bundle = next(step for step in steps(build) if step.get("name") == "Build CLI and installer")
        self.assertEqual(bundle["run"], 'node scripts/build-release.mjs --arch "$ARCH" --version "$VERSION"')
        # The public key is a repository variable; signing keys never reach the build.
        self.assertEqual(bundle["env"], {"TAURI_UPDATER_PUBKEY": "${{ vars.TAURI_UPDATER_PUBKEY }}"})
        self.assertIn("rustup target add", run_text(build))
        package = run_text(build)
        self.assertIn('NAME="purepoint-${VERSION}-${TARGET}"', package)
        self.assertIn('SETUP="PurePoint_${VERSION}_${ARCH}-setup.exe"', package)
        self.assertIn("ZipFile]::CreateFromDirectory", package)
        smoke = next(step for step in steps(build) if "install.ps1" in step.get("run", ""))
        self.assertEqual(smoke["if"], "matrix.arch == 'x64'")
        self.assertIn("-InstallDir $dir", smoke["run"])
        # Windows artifacts never match the Linux jobs' `purepoint-*` download pattern.
        uploads = [step["with"]["name"] for step in steps(build) if step.get("uses", "").startswith("actions/upload-artifact@")]
        self.assertEqual(uploads, ["windows-build-${{ matrix.arch }}"])
        for step in steps(build):
            self.assertNotIn("${{", step.get("run", ""))

    def test_windows_archives_are_signed_with_the_key_both_install_scripts_embed(self):
        job = self.jobs["sign-windows"]
        self.assertEqual(set(job["needs"]), {"version", "windows"})
        sign = next(step for step in steps(job) if step.get("name") == "Sign archives")
        self.assertEqual(sign["env"], {"RELEASE_MINISIGN_KEY": "${{ secrets.RELEASE_MINISIGN_KEY }}"})
        script = sign["run"]
        self.assertIn('if [ -z "$RELEASE_MINISIGN_KEY" ]', script)
        self.assertIn("::warning", script)
        self.assertIn("scripts/install.sh", script)
        self.assertIn("scripts/install.ps1", script)
        self.assertIn(".github/scripts/minisign.sh sign \"$KEY\" dist/*.zip dist/*-setup.exe", script)
        self.assertGreaterEqual(script.count('rm -f "$KEY"'), 2)
        self.assertIn("umask 077", script)
        self.assertIn('"OpenSSL 3."*', script)
        self.assertRegex(INSTALL_PS1.read_text(), r"(?m)^\$ReleasePublicKey = 'RWS[A-Za-z0-9+/]{53}'$")
        ps1_key = re.search(r"(?m)^\$ReleasePublicKey = '(.*)'$", INSTALL_PS1.read_text()).group(1)
        sh_key = re.search(r'(?m)^RELEASE_PUBLIC_KEY="(.*)"$', INSTALL.read_text()).group(1)
        self.assertEqual(ps1_key, sh_key)
        # Checksums are rewritten after Authenticode changes the bytes, before minisign signs them.
        names = [step.get("name") for step in steps(job)]
        self.assertLess(names.index("Authenticode-sign archives and installers"), names.index("Write checksums"))
        self.assertLess(names.index("Write checksums"), names.index("Sign archives"))
        self.assertLess(names.index("Sign archives"), names.index("Sign installers for the updater and write latest-windows.json"))

    def test_authenticode_runs_only_with_a_certificate_and_leaves_none_behind(self):
        job = self.jobs["sign-windows"]
        check = next(step for step in steps(job) if step.get("id") == "authenticode")
        self.assertEqual(check["env"], {"WINDOWS_CERTIFICATE_PFX": "${{ secrets.WINDOWS_CERTIFICATE_PFX }}"})
        self.assertIn("::warning", check["run"])
        sign = next(step for step in steps(job) if step.get("name") == "Authenticode-sign archives and installers")
        self.assertEqual(sign["if"], "steps.authenticode.outputs.enabled == 'true'")
        self.assertIn("signtool", sign["run"])
        self.assertIn("/tr $env:TIMESTAMP_URL", sign["run"])
        self.assertIn("verify /pa", sign["run"])
        self.assertIn("finally", sign["run"])
        self.assertIn("Remove-Item $pfx", sign["run"])
        cleanup = steps(job)[-1]
        self.assertEqual(cleanup["if"], "always()")
        self.assertIn("authenticode.pfx", cleanup["run"])
        self.assertIn("release-minisign.pem", cleanup["run"])

    def test_windows_update_feed_is_signed_checked_and_replaces_only_its_own_asset(self):
        job = self.jobs["sign-windows"]
        feed = next(step for step in steps(job) if step.get("name") == "Sign installers for the updater and write latest-windows.json")
        self.assertEqual(feed["env"]["TAURI_SIGNING_PRIVATE_KEY"], "${{ secrets.TAURI_SIGNING_PRIVATE_KEY }}")
        self.assertEqual(feed["env"]["TAURI_UPDATER_PUBKEY"], "${{ vars.TAURI_UPDATER_PUBKEY }}")
        self.assertIn("node scripts/sign-release-feed.mjs", feed["run"])
        self.assertIn("_x64-setup.exe", feed["run"])
        self.assertIn("_arm64-setup.exe", feed["run"])
        upload = next(step for step in steps(job) if step.get("name") == "Upload Windows update feed")
        self.assertEqual(upload["with"]["name"], "windows-appcast")
        self.assertIn("hashFiles('build/appcast/latest-windows.json')", upload["if"])
        publish = run_text(self.jobs["publish"])
        self.assertIn("if [ -f build/latest-windows.json ]", publish)

    def test_windows_release_is_attested_and_published(self):
        for name in ("attest", "publish"):
            with self.subTest(job=name):
                downloads = [step["with"] for step in steps(self.jobs[name]) if step.get("uses", "").startswith("actions/download-artifact@")]
                self.assertIn({"name": "windows-release", "path": "dist"}, downloads)
        attests = [step["with"] for step in steps(self.jobs["attest"]) if step.get("uses", "").startswith("actions/attest@")]
        for attest in attests:
            with self.subTest(sbom=attest.get("sbom-path")):
                self.assertIn("dist/*-setup.exe", attest["subject-path"])
        provenance = next(attest for attest in attests if "sbom-path" not in attest)
        self.assertIn("dist/*.zip", provenance["subject-path"])
        release = next(step for step in steps(self.jobs["sign-windows"]) if step.get("name") == "Upload Windows release")
        self.assertEqual(release["with"]["name"], "windows-release")
        for pattern in ("dist/*.zip", "dist/*.zip.sha256", "dist/*-setup.exe", "dist/*.minisig"):
            self.assertIn(pattern, release["with"]["path"].split())


if __name__ == "__main__":
    unittest.main()
