"""scripts/install.ps1 installs a download only after verifying who built it (REQ-WINAPP-038).

Runs on Windows only. Releases are served from a local HTTP server; signatures come from a
throwaway key, made by the pure-Python minisign signer below (RFC 8032 Ed25519 over BLAKE2b-512,
independent of the script's own verifier) and, when Git Bash's OpenSSL 3 is available, by the
release signer .github/scripts/minisign.sh itself.
"""

import base64
import functools
import hashlib
import http.server
import os
import shutil
import subprocess
import sys
import tempfile
import threading
import time
import unittest
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
INSTALL = ROOT / "scripts/install.ps1"
INSTALL_SH = ROOT / "scripts/install.sh"
SIGNER = ROOT / ".github/scripts/minisign.sh"
VERSION = "9.9.9"
NAME = f"purepoint-{VERSION}-x86_64-pc-windows-msvc"
WINDOWS = sys.platform == "win32"
PWSH = shutil.which("pwsh") if WINDOWS else None
SYSTEM_ROOT = os.environ.get("SystemRoot", r"C:\Windows")
WINDOWS_POWERSHELL = Path(SYSTEM_ROOT) / "System32/WindowsPowerShell/v1.0/powershell.exe"


# --- minisign, in pure Python (RFC 8032 reference arithmetic) -----------------------------

P = 2**255 - 19
L = 2**252 + 27742317777372353535851937790883648493
D = -121665 * pow(121666, P - 2, P) % P
SQRT_M1 = pow(2, (P - 1) // 4, P)


def _add(p, q):
    a = (p[1] - p[0]) * (q[1] - q[0]) % P
    b = (p[1] + p[0]) * (q[1] + q[0]) % P
    c = 2 * p[3] * q[3] * D % P
    d = 2 * p[2] * q[2] % P
    e, f, g, h = b - a, d - c, d + c, b + a
    return (e * f % P, g * h % P, f * g % P, e * h % P)


def _mul(s, p):
    q = (0, 1, 1, 0)
    while s:
        if s & 1:
            q = _add(q, p)
        p = _add(p, p)
        s >>= 1
    return q


def _recover_x(y, sign):
    x2 = (y * y - 1) * pow(D * y * y + 1, P - 2, P) % P
    x = pow(x2, (P + 3) // 8, P)
    if (x * x - x2) % P:
        x = x * SQRT_M1 % P
    if x & 1 != sign:
        x = P - x
    return x


_BY = 4 * pow(5, P - 2, P) % P
_BX = _recover_x(_BY, 0)
BASE = (_BX, _BY, 1, _BX * _BY % P)


def _compress(p):
    zi = pow(p[2], P - 2, P)
    x, y = p[0] * zi % P, p[1] * zi % P
    return int.to_bytes(y | ((x & 1) << 255), 32, "little")


def _secret(seed):
    h = hashlib.sha512(seed).digest()
    a = int.from_bytes(h[:32], "little")
    a &= (1 << 254) - 8
    a |= 1 << 254
    return a, h[32:]


def ed25519_public(seed):
    return _compress(_mul(_secret(seed)[0], BASE))


def ed25519_sign(seed, message):
    a, prefix = _secret(seed)
    public = _compress(_mul(a, BASE))
    r = int.from_bytes(hashlib.sha512(prefix + message).digest(), "little") % L
    big_r = _compress(_mul(r, BASE))
    k = int.from_bytes(hashlib.sha512(big_r + public + message).digest(), "little") % L
    return big_r + int.to_bytes((r + k * a) % L, 32, "little")


class MinisignKey:
    def __init__(self):
        self.seed = os.urandom(32)
        self.key_id = os.urandom(8)
        self.public = base64.b64encode(b"Ed" + self.key_id + ed25519_public(self.seed)).decode()

    def sign(self, path, trusted=None):
        digest = hashlib.blake2b(Path(path).read_bytes()).digest()
        signature = ed25519_sign(self.seed, digest)
        trusted = trusted or f"timestamp:{int(time.time())}\tfile:{Path(path).name}\thashed"
        global_sig = ed25519_sign(self.seed, signature + trusted.encode())
        Path(f"{path}.minisig").write_bytes((
            "untrusted comment: signature from a test key\n"
            + base64.b64encode(b"ED" + self.key_id + signature).decode() + "\n"
            + f"trusted comment: {trusted}\n"
            + base64.b64encode(global_sig).decode() + "\n").encode())


# --- helpers ----------------------------------------------------------------------------

def git_bash():
    """Git for Windows' bash (never WSL's System32\\bash.exe), when its OpenSSL is 3.x."""
    git = shutil.which("git")
    if not git:
        return None
    for parent in Path(git).resolve().parents:
        bash = parent / "bin/bash.exe"
        if bash.exists():
            version = subprocess.run([str(bash), "-c", "openssl version"], capture_output=True, text=True)
            return str(bash) if version.stdout.startswith("OpenSSL 3") else None
    return None


def embedded_key(path, pattern):
    for line in path.read_text().splitlines():
        if line.startswith(pattern):
            return line.split("'" if path.suffix == ".ps1" else '"')[1]
    raise AssertionError(f"{path.name} embeds no release key")


class QuietHandler(http.server.SimpleHTTPRequestHandler):
    def log_message(self, *args):
        pass


def zone_identifier(path):
    return Path(f"{path}:Zone.Identifier")


@unittest.skipUnless(WINDOWS and PWSH, "needs Windows and pwsh")
class InstallPs1Tests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.server_root = Path(tempfile.mkdtemp(prefix="install-ps1-www-"))
        handler = functools.partial(QuietHandler, directory=str(cls.server_root))
        cls.server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), handler)
        cls.thread = threading.Thread(target=cls.server.serve_forever, daemon=True)
        cls.thread.start()
        cls.base_url = f"http://127.0.0.1:{cls.server.server_address[1]}/download"

    @classmethod
    def tearDownClass(cls):
        cls.server.shutdown()
        cls.server.server_close()
        cls.thread.join()
        shutil.rmtree(cls.server_root, ignore_errors=True)

    def setUp(self):
        self.work = Path(tempfile.mkdtemp(prefix="install-ps1-"))
        self.addCleanup(shutil.rmtree, self.work, ignore_errors=True)
        self.release = self.server_root / "download" / f"v{VERSION}"
        shutil.rmtree(self.release, ignore_errors=True)
        self.release.mkdir(parents=True)
        self.stubs = self.work / "stubs"
        self.stubs.mkdir()
        self.home = self.work / "home"
        self.home.mkdir()
        self.install_dir = self.home / "bin"
        self.key = MinisignKey()
        self.use_key(self.key.public)
        self.build_archive(b"MZ fake pu\n")

    def use_key(self, public):
        self.script = self.work / "install.ps1"
        self.script.write_text(INSTALL.read_text().replace(embedded_key(INSTALL, "$ReleasePublicKey"), public))

    def build_archive(self, pu_body, folder=None):
        archive = (folder or self.release) / f"{NAME}.zip"
        with zipfile.ZipFile(archive, "w") as zf:
            zf.writestr(f"{NAME}/pu.exe", pu_body)
            zf.writestr(f"{NAME}/pu-engine.exe", b"MZ fake engine\n")
            zf.writestr(f"{NAME}/README.md", b"readme\n")
        digest = hashlib.sha256(archive.read_bytes()).hexdigest()
        Path(f"{archive}.sha256").write_text(f"{digest}  {archive.name}\n")
        return archive

    @property
    def archive(self):
        return self.release / f"{NAME}.zip"

    def stub(self, name, body):
        (self.stubs / name).write_text("@echo off\r\n" + body.replace("\n", "\r\n"))

    def install(self, *args, shell=None, env=None, default_dir=False, version=VERSION, base_url=None):
        shell = shell or PWSH
        path = os.pathsep.join([str(self.stubs), str(Path(shell).parent),
                                str(Path(SYSTEM_ROOT) / "System32"), SYSTEM_ROOT])
        # A bare environment: no real gh, minisign or user profile, and a throwaway home.
        environment = {
            "SystemRoot": SYSTEM_ROOT, "SystemDrive": os.environ.get("SystemDrive", "C:"),
            "PATH": path, "PATHEXT": ".COM;.EXE;.BAT;.CMD",
            "USERPROFILE": str(self.home), "HOME": str(self.home),
            "LOCALAPPDATA": str(self.home / "AppData/Local"), "APPDATA": str(self.home / "AppData/Roaming"),
            "TEMP": str(self.work), "TMP": str(self.work),
            "POWERSHELL_TELEMETRY_OPTOUT": "1", "POWERSHELL_UPDATECHECK": "Off",
            **(env or {}),
        }
        command = [shell, "-NoLogo", "-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass",
                   "-File", str(self.script), "-Version", version, "-Arch", "x86_64",
                   "-BaseUrl", base_url or self.base_url]
        if not default_dir:
            command += ["-InstallDir", str(self.install_dir)]
        return subprocess.run(command + list(args), capture_output=True, text=True,
                              env=environment, timeout=180)

    def assertInstalled(self, result, directory=None):
        directory = directory or self.install_dir
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertTrue((directory / "pu.exe").is_file())
        self.assertTrue((directory / "pu-engine.exe").is_file())
        self.assertEqual(sorted(p.name for p in directory.iterdir()), ["pu-engine.exe", "pu.exe"])

    def assertRefused(self, result, message):
        self.assertNotEqual(result.returncode, 0, result.stdout)
        self.assertIn(message, result.stderr)
        self.assertFalse(self.install_dir.exists())

    # --- signatures ---

    def test_installs_a_correctly_signed_release(self):
        self.key.sign(self.archive)
        result = self.install()
        self.assertInstalled(result)
        self.assertIn("verified release signature", result.stdout)
        self.assertEqual((self.install_dir / "pu.exe").read_bytes(), b"MZ fake pu\n")

    @unittest.skipUnless(WINDOWS_POWERSHELL.exists(), "needs Windows PowerShell 5.1")
    def test_windows_powershell_5_1_verifies_too(self):
        self.key.sign(self.archive)
        result = self.install(shell=str(WINDOWS_POWERSHELL))
        self.assertInstalled(result)
        self.assertIn("verified release signature", result.stdout)

    def test_verifies_signatures_made_by_the_release_signer(self):
        bash = git_bash()
        if not bash:
            self.skipTest("needs Git Bash with OpenSSL 3")
        pem = self.work / "release.pem"
        run = functools.partial(subprocess.run, check=True, capture_output=True, text=True)
        public = run([bash, SIGNER.as_posix(), "keygen", pem.as_posix()]).stdout.strip()
        run([bash, SIGNER.as_posix(), "sign", pem.as_posix(), self.archive.as_posix()])
        self.use_key(public)
        self.assertInstalled(self.install())

    def test_signatures_verify_with_minisign_when_it_is_on_path(self):
        minisign = shutil.which("minisign")
        if not minisign:
            self.skipTest("minisign not installed")
        self.key.sign(self.archive)
        shutil.copy(minisign, self.stubs / "minisign.exe")
        self.assertInstalled(self.install())

    def test_refuses_an_archive_changed_after_signing(self):
        self.key.sign(self.archive)
        self.build_archive(b"MZ replaced\n")
        self.assertRefused(self.install(), "signature check failed")

    def test_refuses_a_signature_from_another_key(self):
        MinisignKey().sign(self.archive)
        self.assertRefused(self.install(), "signature check failed")

    def test_refuses_a_forged_signature_under_the_right_key_id(self):
        other = MinisignKey()
        other.key_id = self.key.key_id
        other.sign(self.archive)
        self.assertRefused(self.install(), "signature check failed")

    def test_refuses_an_edited_trusted_comment(self):
        self.key.sign(self.archive)
        signature = Path(f"{self.archive}.minisig")
        lines = signature.read_text().splitlines()
        lines[2] = lines[2].replace("hashed", "hashed\tedited")
        signature.write_text("\n".join(lines) + "\n")
        self.assertRefused(self.install(), "signature check failed")

    def test_refuses_a_checksum_mismatch(self):
        self.key.sign(self.archive)
        Path(f"{self.archive}.sha256").write_text("0" * 64 + f"  {NAME}.zip\n")
        self.assertRefused(self.install(), "checksum mismatch")

    def test_refuses_an_unsigned_release(self):
        result = self.install()
        self.assertRefused(result, "could not verify")
        self.assertIn("the release has no signature", result.stderr)

    def test_opt_out_installs_an_unsigned_release_with_a_warning(self):
        result = self.install(env={"PU_INSTALL_SKIP_VERIFY": "1"})
        self.assertInstalled(result)
        self.assertIn("warning: PU_INSTALL_SKIP_VERIFY=1", result.stderr)

    def test_opt_out_never_overrides_a_failed_signature(self):
        self.key.sign(self.archive)
        self.build_archive(b"MZ replaced\n")
        self.assertRefused(self.install(env={"PU_INSTALL_SKIP_VERIFY": "1"}), "signature check failed")

    # --- build provenance ---

    def test_gh_attestation_alone_verifies_an_unsigned_release(self):
        self.stub("gh.cmd", 'if "%1"=="auth" exit /b 0\nif "%1 %2"=="attestation verify" exit /b 0\nexit /b 1\n')
        result = self.install()
        self.assertInstalled(result)
        self.assertIn("verified build provenance", result.stdout)

    def test_failed_gh_attestation_is_fatal_even_with_a_good_signature(self):
        self.key.sign(self.archive)
        self.stub("gh.cmd", 'if "%1"=="auth" exit /b 0\nexit /b 1\n')
        self.assertRefused(self.install(env={"PU_INSTALL_SKIP_VERIFY": "1"}), "gh attestation verify failed")

    def test_signed_out_gh_is_skipped(self):
        self.key.sign(self.archive)
        self.stub("gh.cmd", "exit /b 1\n")
        self.assertInstalled(self.install())

    # --- installing ---

    def test_local_archive_is_unblocked_and_not_verified(self):
        folder = self.work / "downloads"
        folder.mkdir()
        archive = self.build_archive(b"MZ local\n", folder)
        mark = b"[ZoneTransfer]\r\nZoneId=3\r\n"
        zone_identifier(archive).write_bytes(mark)
        result = self.install(env={"PU_ARCHIVE": str(archive)})
        self.assertInstalled(result)
        self.assertIn("a local archive is not verified", result.stdout)
        self.assertIn("Unblock-File", result.stdout)
        for exe in ("pu.exe", "pu-engine.exe"):
            with self.subTest(exe=exe):
                self.assertFalse(zone_identifier(self.install_dir / exe).exists())
        # The user's own file is left as it was.
        self.assertEqual(zone_identifier(archive).read_bytes(), mark)

    def test_each_installed_binary_loses_its_mark_of_the_web(self):
        # Expand-Archive does not carry the zip's mark onto its entries, so the per-binary
        # Unblock-File is checked on Install-Binary itself, loaded from the script's AST.
        source = self.work / "pu.exe"
        source.write_bytes(b"MZ marked\n")
        zone_identifier(source).write_bytes(b"[ZoneTransfer]\r\nZoneId=3\r\n")
        self.install_dir.mkdir()
        loader = (
            "$ast = [System.Management.Automation.Language.Parser]::ParseFile($args[0], [ref]$null, [ref]$null);"
            "$fn = $ast.Find({ param($n) $n -is [System.Management.Automation.Language.FunctionDefinitionAst]"
            " -and $n.Name -eq 'Install-Binary' }, $true);"
            "function Say([string]$m) { Write-Host $m };"
            ". ([scriptblock]::Create($fn.Extent.Text)); Install-Binary $args[1] $args[2]"
        )
        script = self.stubs / "load-install-binary.ps1"
        script.write_text(loader)
        result = subprocess.run([PWSH, "-NoLogo", "-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass",
                                 "-File", str(script), str(INSTALL), str(source), str(self.install_dir)],
                                capture_output=True, text=True, timeout=120)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertEqual((self.install_dir / "pu.exe").read_bytes(), b"MZ marked\n")
        self.assertFalse(zone_identifier(self.install_dir / "pu.exe").exists())

    def test_defaults_to_the_users_pu_bin_and_prints_the_path_step(self):
        self.key.sign(self.archive)
        result = self.install(default_dir=True)
        self.assertInstalled(result, self.home / ".pu/bin")
        self.assertIn("add it to your PATH", result.stdout)
        self.assertIn("-AddToPath", result.stdout)

    def test_replaces_a_running_daemon_by_renaming_it_aside(self):
        self.key.sign(self.archive)
        self.install_dir.mkdir()
        engine = self.install_dir / "pu-engine.exe"
        shutil.copy(Path(SYSTEM_ROOT) / "System32/PING.EXE", engine)
        running = subprocess.Popen([str(engine), "-n", "60", "127.0.0.1"],
                                   stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        try:
            time.sleep(0.5)
            if running.poll() is not None:
                self.skipTest("a copied PING.EXE does not run here")
            result = self.install()
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            self.assertEqual(engine.read_bytes(), b"MZ fake engine\n")
            aside = [p.name for p in self.install_dir.iterdir() if p.name.startswith("pu-engine.exe.old.")]
            self.assertEqual(len(aside), 1)
            self.assertIn("still in use", result.stdout)
        finally:
            running.kill()
            running.wait()

    def test_refuses_a_malformed_version(self):
        result = self.install(version="1.2.3; rm")
        self.assertRefused(result, "release version must be X.Y.Z")

    def test_refuses_plain_http_to_another_host(self):
        self.key.sign(self.archive)
        self.assertRefused(self.install(base_url="http://example.invalid/download"), "refusing to download over http")

    def test_embeds_the_same_release_key_as_install_sh(self):
        self.assertEqual(embedded_key(INSTALL, "$ReleasePublicKey"), embedded_key(INSTALL_SH, "RELEASE_PUBLIC_KEY="))


if __name__ == "__main__":
    unittest.main()
