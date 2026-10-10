import {
  mkdir,
  lstat,
  readFile,
  writeFile,
  rename,
  rm,
} from "node:fs/promises";
import path from "node:path";
import { randomUUID } from "node:crypto";
import { acquirePrivateLock } from "./lock-helper.js";

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
export async function writePrivateJSON(file, value, assertHeld = () => {}) {
  const temporary = file + "." + randomUUID() + ".tmp";
  try {
    assertHeld();
    await writeFile(temporary, JSON.stringify(value) + "\n", {
      mode: 0o600,
      flag: "wx",
    });
    assertHeld();
    await rename(temporary, file);
    assertHeld();
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
  {
    helperPath = process.env.POINT_GUARD_LOCK_HELPER_PATH,
    onLost = undefined,
  } = {},
) {
  await privateDirectory(directory);
  const lock = path.join(directory, "runtime.lock");
  const holder = await acquirePrivateLock({ file: lock, helperPath, onLost });
  const assertHeld = () => holder.assertHeld();
  let closed = false;
  let serial = Promise.resolve();
  const close = async () => {
    if (closed) return;
    closed = true;
    await serial.catch(() => {});
    await holder.release();
  };
  try {
    assertHeld();
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
    await writePrivateJSON(file, value, assertHeld);
    return {
      directory,
      assertHeld,
      get held() {
        return holder.held;
      },
      get value() {
        return value;
      },
      close,
      save(patch) {
        const operation = serial.then(async () => {
          if (closed) throw new Error("Runtime state closed.");
          const next = { ...value, ...patch };
          assertHeld();
          validate(next);
          await writePrivateJSON(file, next, assertHeld);
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
