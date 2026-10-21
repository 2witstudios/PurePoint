"""scripts/install.sh installs a download only after verifying who built it."""

import os
import shutil
import subprocess
import tarfile
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
INSTALL = ROOT / "scripts/install.sh"
SIGNER = ROOT / ".github/scripts/minisign.sh"
VERSION = "9.9.9"
NAME = f"purepoint-{VERSION}-x86_64-unknown-linux-musl"
BASE = f"https://github.com/2witstudios/PurePoint/releases/download/v{VERSION}"
SYSTEM_PATH = "/usr/bin:/bin:/usr/sbin:/sbin"


def openssl():
    for candidate in (shutil.which("openssl"), "/opt/homebrew/bin/openssl", "/usr/local/bin/openssl"):
        if candidate and Path(candidate).exists():
            version = subprocess.run([candidate, "version"], capture_output=True, text=True).stdout
            if version.startswith("OpenSSL 3"):
                return candidate
    return None


OPENSSL = openssl()


def write_executable(path, text):
    path.write_text(text)
    path.chmod(0o755)


@unittest.skipUnless(OPENSSL, "needs OpenSSL 3")
class InstallScriptTests(unittest.TestCase):
    def setUp(self):
        self.work = Path(tempfile.mkdtemp(prefix="install-test-"))
        self.addCleanup(shutil.rmtree, self.work)
        self.bin = self.work / "bin"
        self.bin.mkdir()
        os.symlink(OPENSSL, self.bin / "openssl")
        self.release = self.work / "release"
        self.release.mkdir()
        self.home = self.work / "home"
        self.home.mkdir()
        self.key = self.work / "release.pem"
        self.public_key = self.sign_tool("keygen", self.key).strip()
        self.script = self.work / "install.sh"
        self.script.write_text(INSTALL.read_text().replace(
            self.embedded_key(), self.public_key))

        write_executable(self.bin / "uname", '#!/bin/sh\ncase "$1" in -s) echo Linux ;; -m) echo x86_64 ;; esac\n')
        # Serves release URLs from the local release directory; anything else 404s.
        write_executable(self.bin / "curl", f"""#!/bin/sh
url= out=
while [ $# -gt 0 ]; do
    case "$1" in -o) out=$2; shift ;; -*) ;; *) url=$1 ;; esac
    shift
done
case "$url" in {BASE}/*) file="{self.release}/${{url#{BASE}/}}" ;; *) exit 22 ;; esac
[ -f "$file" ] || exit 22
cp "$file" "$out"
""")
        self.build_archive(b"#!/bin/sh\necho pu\n")

    def embedded_key(self):
        for line in INSTALL.read_text().splitlines():
            if line.startswith("RELEASE_PUBLIC_KEY="):
                return line.split('"')[1]
        self.fail("install.sh embeds no RELEASE_PUBLIC_KEY")

    def sign_tool(self, *args):
        env = {**os.environ, "PATH": f"{self.bin}:{SYSTEM_PATH}"}
        return subprocess.run(["bash", str(SIGNER), *map(str, args)], check=True,
                              capture_output=True, text=True, env=env).stdout

    def build_archive(self, pu_body):
        stage = self.work / "stage" / NAME
        shutil.rmtree(stage.parent, ignore_errors=True)
        stage.mkdir(parents=True)
        for binary, body in (("pu", pu_body), ("pu-engine", b"#!/bin/sh\necho engine\n")):
            (stage / binary).write_bytes(body)
        archive = self.release / f"{NAME}.tar.gz"
        with tarfile.open(archive, "w:gz") as tar:
            tar.add(stage, arcname=NAME)
        self.write_checksum()
        return archive

    def write_checksum(self):
        archive = self.release / f"{NAME}.tar.gz"
        digest = subprocess.run([OPENSSL, "dgst", "-sha256", "-r", str(archive)], check=True,
                                capture_output=True, text=True).stdout.split()[0]
        (self.release / f"{NAME}.tar.gz.sha256").write_text(f"{digest}  {NAME}.tar.gz\n")

    def sign(self, key=None):
        self.sign_tool("sign", key or self.key, self.release / f"{NAME}.tar.gz")

    def install(self, **env):
        environment = {
            "HOME": str(self.home), "PATH": f"{self.bin}:{SYSTEM_PATH}",
            "PU_VERSION": VERSION, "PU_INSTALL_DIR": str(self.home / "bin"), **env,
        }
        return subprocess.run(["sh", str(self.script)], capture_output=True, text=True, env=environment)

    def assertInstalled(self, result):
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue((self.home / "bin/pu").exists())
        self.assertTrue((self.home / "bin/pu-engine").exists())

    def assertRefused(self, result, message):
        self.assertNotEqual(result.returncode, 0, result.stdout)
        self.assertIn(message, result.stderr)
        self.assertFalse((self.home / "bin").exists())

    def test_installs_a_correctly_signed_release(self):
        self.sign()
        result = self.install()
        self.assertInstalled(result)
        self.assertIn("verified release signature", result.stdout)

    def test_signatures_verify_with_minisign_when_it_is_installed(self):
        minisign = shutil.which("minisign")
        if not minisign:
            self.skipTest("minisign not installed")
        self.sign()
        archive = self.release / f"{NAME}.tar.gz"
        subprocess.run([minisign, "-Vm", str(archive), "-P", self.public_key], check=True, capture_output=True)
        os.symlink(minisign, self.bin / "minisign")
        self.assertInstalled(self.install())

    def test_refuses_an_archive_changed_after_signing(self):
        self.sign()
        self.build_archive(b"#!/bin/sh\necho replaced\n")
        self.assertRefused(self.install(), "signature check failed")

    def test_refuses_a_signature_from_another_key(self):
        other = self.work / "other.pem"
        self.sign_tool("keygen", other)
        self.sign(other)
        self.assertRefused(self.install(), "signature check failed")

    def test_refuses_an_edited_trusted_comment(self):
        self.sign()
        signature = self.release / f"{NAME}.tar.gz.minisig"
        lines = signature.read_text().splitlines()
        lines[2] = lines[2].replace("hashed", "hashed\tedited")
        signature.write_text("\n".join(lines) + "\n")
        self.assertRefused(self.install(), "signature check failed")

    def test_refuses_an_unsigned_release(self):
        self.assertRefused(self.install(), "could not verify")

    def test_opt_out_installs_an_unsigned_release_with_a_warning(self):
        result = self.install(PU_INSTALL_SKIP_VERIFY="1")
        self.assertInstalled(result)
        self.assertIn("PU_INSTALL_SKIP_VERIFY=1", result.stderr)

    def test_opt_out_never_overrides_a_failed_signature(self):
        self.sign()
        self.build_archive(b"#!/bin/sh\necho replaced\n")
        self.assertRefused(self.install(PU_INSTALL_SKIP_VERIFY="1"), "signature check failed")

    def test_gh_attestation_alone_verifies_an_unsigned_release(self):
        write_executable(self.bin / "gh", '#!/bin/sh\n[ "$1" = auth ] && exit 0\n[ "$1 $2" = "attestation verify" ] && exit 0\nexit 1\n')
        result = self.install()
        self.assertInstalled(result)
        self.assertIn("verified build provenance", result.stdout)

    def test_failed_gh_attestation_is_fatal_even_with_a_good_signature(self):
        self.sign()
        write_executable(self.bin / "gh", '#!/bin/sh\n[ "$1" = auth ] && exit 0\nexit 1\n')
        self.assertRefused(self.install(PU_INSTALL_SKIP_VERIFY="1"), "gh attestation verify failed")

    def test_signed_out_gh_is_skipped(self):
        self.sign()
        write_executable(self.bin / "gh", "#!/bin/sh\nexit 1\n")
        self.assertInstalled(self.install())


if __name__ == "__main__":
    unittest.main()
