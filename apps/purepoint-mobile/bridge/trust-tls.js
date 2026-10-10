import { mkdir, lstat, readFile, rename, rm, mkdtemp } from "node:fs/promises";
import path from "node:path";
import { execFile } from "node:child_process";
import { promisify } from "node:util";
import {
  X509Certificate,
  createPrivateKey,
  createPublicKey,
  createHash,
} from "node:crypto";
const execute = promisify(execFile);
export async function privateEntry(file, directory = false) {
  const info = await lstat(file);
  if (
    info.isSymbolicLink() ||
    (directory ? !info.isDirectory() : !info.isFile()) ||
    info.mode & 0o077 ||
    (typeof process.getuid === "function" && info.uid !== process.getuid())
  )
    throw new Error(
      "Point Guard trust state must be owner-only regular files and a private directory.",
    );
  return info;
}
async function exists(file) {
  try {
    await lstat(file);
    return true;
  } catch (e) {
    if (e.code === "ENOENT") return false;
    throw e;
  }
}
/** Provision once under the trust writer lock; partial/expired identity requires deliberate recovery. */
export async function ensureHostTLS(directory, assertHeld = () => {}) {
  assertHeld();
  await mkdir(directory, { recursive: true, mode: 0o700 });
  await privateEntry(directory, true);
  const certFile = path.join(directory, "identity-cert.pem");
  const keyFile = path.join(directory, "identity-key.pem");
  const present = await Promise.all([
    exists(certFile),
    exists(keyFile),
    exists(path.join(directory, "trust.json")),
  ]);
  const created = !present[0] && !present[1] && !present[2];
  assertHeld();
  if (created) {
    const temporary = await mkdtemp(path.join(directory, ".identity-"));
    try {
      // Absolute OS tool, fixed arguments, no shell or caller-controlled certificate fields.
      assertHeld();
      await execute(
        "/usr/bin/openssl",
        [
          "req",
          "-x509",
          "-newkey",
          "rsa:2048",
          "-nodes",
          "-sha256",
          "-days",
          "3650",
          "-subj",
          "/CN=Point Guard",
          "-keyout",
          path.join(temporary, "key.pem"),
          "-out",
          path.join(temporary, "cert.pem"),
        ],
        { timeout: 15000, maxBuffer: 65536 },
      );
      const { chmod } = await import("node:fs/promises");
      await chmod(path.join(temporary, "key.pem"), 0o600);
      await chmod(path.join(temporary, "cert.pem"), 0o600);
      assertHeld();
      await rename(path.join(temporary, "key.pem"), keyFile);
      assertHeld();
      await rename(path.join(temporary, "cert.pem"), certFile);
    } catch {
      throw new Error(
        "Could not provision Point Guard TLS identity with /usr/bin/openssl. Check private state permissions; partial identity needs deliberate recovery.",
      );
    } finally {
      await rm(temporary, { recursive: true, force: true });
    }
  } else if (!present[0] || !present[1])
    throw new Error(
      "Point Guard TLS identity is incomplete. Restore private identity files or deliberately reset trust and re-pair devices.",
    );
  await privateEntry(certFile);
  await privateEntry(keyFile);
  const cert = await readFile(certFile);
  const key = await readFile(keyFile);
  try {
    const identity = new X509Certificate(cert);
    if (
      Date.parse(identity.validFrom) > Date.now() ||
      Date.parse(identity.validTo) <= Date.now()
    )
      throw new Error("expired");
    const actualKey = createPublicKey(createPrivateKey(key)).export({
      type: "spki",
      format: "der",
    });
    const expectedKey = identity.publicKey.export({
      type: "spki",
      format: "der",
    });
    if (!actualKey.equals(expectedKey)) throw new Error("mismatch");
    return {
      cert,
      key,
      created,
      certificateSHA256: createHash("sha256")
        .update(identity.raw)
        .digest("hex"),
    };
  } catch {
    throw new Error(
      "Point Guard TLS identity is invalid, expired or mismatched. Restore the original identity or deliberately reset trust and re-pair devices.",
    );
  }
}
