import { open, lstat, access } from "node:fs/promises";
import { constants } from "node:fs";
import { spawn } from "node:child_process";
import { randomUUID } from "node:crypto";
import path from "node:path";
import { assertPrivate } from "./acl.js";
const win32 = process.platform === "win32";
/** libuv on Windows: open with no sharing, so no other open succeeds while held. */
const UV_FS_O_EXLOCK = 0x10000000;
export class LockError extends Error {
  constructor(code) {
    super(
      code === "lock_busy"
        ? "Point Guard private state is owned by another running service."
        : code === "lock_private"
          ? "Point Guard lock state must be private and owned by this user."
          : code === "lock_lost"
            ? "Point Guard lock holder exited. Owned writes are fenced until cleanup drains."
            : "The packaged Point Guard lock helper could not establish private ownership.",
    );
    this.code = code;
  }
}
function privateMetadata(s, directory = false) {
  return (
    !s.isSymbolicLink() &&
    (directory ? s.isDirectory() : s.isFile() && s.nlink === 1) &&
    (s.mode & 0o777) === (directory ? 0o700 : 0o600) &&
    s.uid === process.getuid()
  );
}
/**
 * Windows has no mode bits or uids: a plain, singly linked entry (never a
 * symlink or junction) with an owner-only ACL (acl.js) instead.
 */
async function privateEntry(target, s, directory = false) {
  if (!win32) return privateMetadata(s, directory);
  if (
    s.isSymbolicLink() ||
    !(directory ? s.isDirectory() : s.isFile() && s.nlink === 1)
  )
    return false;
  try {
    await assertPrivate(target, { directory });
    return true;
  } catch {
    return false;
  }
}
/**
 * Retain the parent OFD even on helper loss, until caller IO drain and explicit release.
 *
 * Windows: the file is opened without sharing (the exclusion a second owner
 * meets, held by this handle until release) and passed as descriptor 3; the
 * helper takes `LockFileEx` on it and runs the same private-state checks.
 */
export async function acquirePrivateLock({
  file,
  helperPath = process.env.POINT_GUARD_LOCK_HELPER_PATH,
  onLost = undefined,
}) {
  if (!helperPath || !path.isAbsolute(helperPath))
    throw new LockError("lock_helper_missing");
  try {
    await access(helperPath, constants.X_OK);
  } catch {
    throw new LockError("lock_helper_missing");
  }
  if (!path.isAbsolute(file)) throw new LockError("lock_private");
  let handle;
  try {
    const directory = path.dirname(file);
    if (!(await privateEntry(directory, await lstat(directory), true)))
      throw new LockError("lock_private");
    // O_NONBLOCK avoids opening an attacker-replaced FIFO indefinitely. libuv sets CLOEXEC.
    try {
      handle = await open(
        file,
        win32
          ? constants.O_RDWR | constants.O_CREAT | UV_FS_O_EXLOCK
          : constants.O_RDWR |
              constants.O_CREAT |
              constants.O_NOFOLLOW |
              constants.O_NONBLOCK,
        0o600,
      );
    } catch (error) {
      // Windows: a sharing violation is another owner's no-sharing handle.
      throw win32 && error.code === "EBUSY"
        ? new LockError("lock_busy")
        : error;
    }
    if (win32) {
      const opened = await handle.stat();
      if (
        !opened.isFile() ||
        opened.nlink !== 1 ||
        !(await privateEntry(file, await lstat(file), false))
      )
        throw new LockError("lock_private");
    } else if (
      !privateMetadata(await handle.stat()) ||
      !privateMetadata(await lstat(file))
    )
      throw new LockError("lock_private");
  } catch (error) {
    await handle?.close();
    throw error instanceof LockError ? error : new LockError("lock_private");
  }
  // Windows file indexes can exceed 2^53: compare them exactly.
  const metadata = await handle.stat(win32 ? { bigint: true } : undefined);
  const nonce = randomUUID();
  const child = spawn(
    helperPath,
    ["--file", file, "--nonce", nonce, "--owner-pid", String(process.pid)],
    {
      stdio: ["pipe", "pipe", "pipe", handle.fd],
      env: win32
        ? { SystemRoot: process.env.SystemRoot ?? "C:\\Windows" }
        : { PATH: "/usr/bin:/bin" },
      windowsHide: true,
    },
  );
  let held = false,
    releasing = false,
    acquired = false,
    lossReported = false;
  let resolveLost;
  const lost = new Promise((resolve) => {
    resolveLost = resolve;
  });
  const reportLoss = () => {
    held = false;
    if (releasing || !acquired || lossReported) return;
    lossReported = true;
    const error = new LockError("lock_lost");
    resolveLost(error);
    try {
      onLost?.(error);
    } catch {
      /* Caller still receives lost; retained FD is never prematurely closed. */
    }
  };
  let closeResolve;
  const closed = new Promise((resolve) => {
    closeResolve = resolve;
  });
  child.once("exit", reportLoss);
  child.stdin.on("error", () => {});
  child.once("close", () => {
    reportLoss();
    closeResolve();
  });
  child.stderr.on("data", () => {}); // Raw helper stderr is never logged/returned.
  let releasePromise;
  const release = () => {
    if (releasePromise) return releasePromise;
    releasing = true;
    held = false;
    releasePromise = (async () => {
      child.stdin.end();
      await closed;
      await handle.close(); // last-close only, after caller's submitted IO has drained
    })();
    return releasePromise;
  };
  try {
    await new Promise((resolve, reject) => {
      let output = "";
      let settled = false;
      const timer = setTimeout(
        () => finish(new LockError("lock_helper_failed")),
        30000,
      );
      const finish = (error) => {
        if (settled) return;
        settled = true;
        clearTimeout(timer);
        error ? reject(error) : resolve(undefined);
      };
      child.once("error", () => finish(new LockError("lock_helper_missing")));
      child.once("exit", () => finish(new LockError("lock_helper_failed")));
      child.stdout.on("data", (chunk) => {
        if (settled) return;
        output += chunk.toString();
        if (output.length > 1024)
          return finish(new LockError("lock_helper_failed"));
        if (!output.includes("\n")) return;
        let r;
        try {
          r = JSON.parse(output.trim());
        } catch {
          return finish(new LockError("lock_helper_failed"));
        }
        if (
          r.schemaVersion === 1 &&
          r.type === "error" &&
          [
            "lock_busy",
            "lock_private",
            "lock_path_changed",
            "lock_helper_failed",
          ].includes(r.code)
        )
          return finish(new LockError(r.code));
        if (
          r.schemaVersion !== 1 ||
          r.type !== "ready" ||
          r.nonce !== nonce ||
          r.pid !== child.pid ||
          r.device !== String(metadata.dev) ||
          r.inode !== String(metadata.ino)
        )
          return finish(new LockError("lock_helper_failed"));
        acquired = true;
        held = true;
        finish();
      });
    });
    if (child.exitCode !== null || child.signalCode !== null) {
      reportLoss();
      throw new LockError("lock_helper_failed");
    }
  } catch (error) {
    await release();
    throw error;
  }
  return {
    get held() {
      return held;
    },
    assertHeld() {
      if (!held) throw new LockError("lock_lost");
    },
    release,
    holderPid: child.pid,
    identity: { device: String(metadata.dev), inode: String(metadata.ino) },
    lost,
  };
}
