import {
  mkdir,
  lstat,
  readFile,
  rename,
  rm,
  mkdtemp,
  writeFile,
  chmod,
} from "node:fs/promises";
import path from "node:path";
import { mintHostIdentity } from "./identity.js";
import {
  X509Certificate,
  createPrivateKey,
  createPublicKey,
  createHash,
} from "node:crypto";
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
      // In-process minting: no OS tool, PATH lookup or caller-controlled fields.
      assertHeld();
      const minted = mintHostIdentity();
      await writeFile(path.join(temporary, "key.pem"), minted.key, {
        mode: 0o600,
        flag: "wx",
      });
      await writeFile(path.join(temporary, "cert.pem"), minted.cert, {
        mode: 0o600,
        flag: "wx",
      });
      // Modes are explicit, independent of the process umask.
      await chmod(path.join(temporary, "key.pem"), 0o600);
      await chmod(path.join(temporary, "cert.pem"), 0o600);
      assertHeld();
      await rename(path.join(temporary, "key.pem"), keyFile);
      assertHeld();
      await rename(path.join(temporary, "cert.pem"), certFile);
    } catch {
      throw new Error(
        "Could not provision Point Guard TLS identity. Check private state permissions; partial identity needs deliberate recovery.",
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
