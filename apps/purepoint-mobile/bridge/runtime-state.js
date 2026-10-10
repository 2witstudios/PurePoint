import {
  mkdir,
  lstat,
  readFile,
  writeFile,
  rename,
  rm,
  open,
} from "node:fs/promises";
import path from "node:path";
import { randomUUID } from "node:crypto";

/** Reject symlinks/public state rather than silently changing owner-managed data. */
export async function privatePath(file, directory = false) {
  const s = await lstat(file);
  if (
    s.isSymbolicLink() ||
    (directory ? !s.isDirectory() : !s.isFile()) ||
    (s.mode & 0o077) !== 0 ||
    (typeof process.getuid === "function" && s.uid !== process.getuid())
  )
    throw new Error(
      "Point Guard state must be private and owned by this user.",
    );
  return s;
}
export async function privateDirectory(dir) {
  await mkdir(dir, { recursive: true, mode: 0o700 });
  await privatePath(dir, true);
}
export async function writePrivateJSON(file, value) {
  const temporary = file + "." + randomUUID() + ".tmp";
  try {
    await writeFile(temporary, JSON.stringify(value) + "\n", {
      mode: 0o600,
      flag: "wx",
    });
    await rename(temporary, file);
  } finally {
    await rm(temporary, { force: true });
  }
}
function validate(value) {
  if (
    value?.schemaVersion !== 1 ||
    typeof value.desktopClientId !== "string" ||
    !/^[a-zA-Z0-9_-]{1,128}$/.test(value.desktopClientId)
  )
    throw new Error(
      "Unsupported or corrupt Point Guard state. Restore a known backup.",
    );
  for (const key of ["selectedSessionPath", "provider", "model", "cwd"])
    if (
      value[key] !== undefined &&
      (typeof value[key] !== "string" || value[key].length > 8192)
    )
      throw new Error("Corrupt Point Guard selection. Restore a known backup.");
}
/** Exclusive app-owned startup. Unknown/stale locks are never removed or adopted. */
/** @param {string} directory @param {string} [clientId] @param {string} [instanceId] */
export async function openRuntimeState(
  directory,
  clientId,
  instanceId = randomUUID(),
) {
  await privateDirectory(directory);
  const lock = path.join(directory, "runtime.lock");
  let handle;
  try {
    handle = await open(lock, "wx", 0o600);
  } catch (e) {
    if (e.code === "EEXIST")
      throw new Error(
        "Point Guard is already running or has an unresolved startup lock. Quit the owning app; inspect the private lock before deliberate recovery.",
      );
    throw e;
  }
  let closed = false;
  let serial = Promise.resolve();
  const close = async () => {
    if (closed) return;
    closed = true;
    await serial.catch(() => {});
    await handle.close();
    await rm(lock);
  };
  try {
    await handle.writeFile(JSON.stringify({ pid: process.pid, instanceId }));
    const file = path.join(directory, "runtime.json");
    let value;
    try {
      const s = await privatePath(file);
      if (s.size > 65536) throw new Error("Oversized Point Guard state.");
      value = JSON.parse(await readFile(file, "utf8"));
    } catch (e) {
      if (e.code !== "ENOENT") throw e;
      value = { schemaVersion: 1, desktopClientId: clientId ?? randomUUID() };
    }
    validate(value);
    await writePrivateJSON(file, value);
    return {
      directory,
      get value() {
        return value;
      },
      close,
      save(patch) {
        const operation = serial.then(async () => {
          if (closed) throw new Error("Runtime state closed.");
          const next = { ...value, ...patch };
          validate(next);
          await writePrivateJSON(file, next);
          value = next;
        });
        serial = operation.catch(() => {});
        return operation;
      },
    };
  } catch (e) {
    await close();
    throw e;
  }
}
