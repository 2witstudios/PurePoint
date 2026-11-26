import test from "node:test";
import assert from "node:assert/strict";
import {
  mkdir,
  mkdtemp,
  readFile,
  writeFile,
  stat,
  rm,
  symlink,
} from "node:fs/promises";
import path from "node:path";
import os from "node:os";
import { execFileSync } from "node:child_process";
import { acquirePrivateLock } from "./lock-helper.js";
import { makePrivate } from "./acl.js";
const win32 = process.platform === "win32";
// POSIX symlinks and modes; the Windows variant below uses ACLs and junctions.
const unixOnly = { skip: win32 && "POSIX symlinks and modes (Unix only)" };
const windowsOnly = { skip: !win32 && "Windows ACLs and junctions" };
const helperPath = process.env.POINT_GUARD_LOCK_HELPER_PATH;
/** mkdtemp is 0700 on Unix; on Windows it inherits TEMP's ACL until made private. */
async function privateTemp(prefix) {
  const directory = await mkdtemp(path.join(os.tmpdir(), prefix));
  if (win32) await makePrivate(directory);
  return directory;
}
test("given owned helper crash should retain kernel exclusion until owner IO drains and releases", async () => {
  const directory = await privateTemp("pointguard-ofd-");
  let first;
  try {
    const file = path.join(directory, "lock");
    await writeFile(file, "old private PID metadata", { mode: 0o600 });
    const inode = (await stat(file)).ino;
    let lost = 0;
    first = await acquirePrivateLock({
      file,
      helperPath,
      onLost: () => lost++,
    });
    process.kill(first.holderPid, "SIGKILL"); // directly owned temporary fixture only
    await first.lost;
    assert.equal(first.held, false);
    assert.equal(lost, 1);
    assert.throws(
      () => first.assertHeld(),
      (e) => e.code === "lock_lost",
    );
    await assert.rejects(
      acquirePrivateLock({ file, helperPath }),
      (e) => e.code === "lock_busy",
    );
    await first.release();
    const second = await acquirePrivateLock({
      file,
      helperPath,
      onLost: () => lost++,
    });
    await second.release();
    await second.release();
    assert.equal(lost, 1);
    assert.equal((await stat(file)).ino, inode);
    assert.equal(await readFile(file, "utf8"), "old private PID metadata");
  } finally {
    await first?.release();
    await rm(directory, { recursive: true, force: true });
  }
});
test("given symlink or public inode should refuse without repair", unixOnly, async () => {
  const directory = await mkdtemp(
    path.join(os.tmpdir(), "pointguard-lock-private-"),
  );
  try {
    const target = path.join(directory, "target"),
      link = path.join(directory, "link");
    await writeFile(target, "unchanged", { mode: 0o644 });
    await symlink(target, link);
    for (const file of [target, link])
      await assert.rejects(
        acquirePrivateLock({ file, helperPath }),
        (e) => e.code === "lock_private",
      );
    assert.equal(await readFile(target, "utf8"), "unchanged");
    assert.equal((await stat(target)).mode & 0o777, 0o644);
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
});
test("given a shared ACL or a junction should refuse without repair", windowsOnly, async () => {
  const directory = await privateTemp("pointguard-lock-private-");
  try {
    const target = path.join(directory, "target");
    await writeFile(target, "unchanged");
    const icacls = path.join(process.env.SystemRoot, "System32", "icacls.exe");
    execFileSync(icacls, [target, "/grant", "*S-1-1-0:R"]); // Everyone
    await assert.rejects(
      acquirePrivateLock({ file: target, helperPath }),
      (e) => e.code === "lock_private",
    );
    const inner = path.join(directory, "inner");
    await mkdir(inner);
    await makePrivate(inner);
    const junction = path.join(directory, "junction");
    await symlink(inner, junction, "junction");
    await assert.rejects(
      acquirePrivateLock({ file: path.join(junction, "lock"), helperPath }),
      (e) => e.code === "lock_private",
    );
    assert.equal(await readFile(target, "utf8"), "unchanged");
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
});
test("given a live holder should report busy, then hand over after release", windowsOnly, async () => {
  const directory = await privateTemp("pointguard-lock-busy-");
  try {
    const file = path.join(directory, "runtime.lock");
    const first = await acquirePrivateLock({ file, helperPath });
    assert.equal(first.held, true);
    await assert.rejects(
      acquirePrivateLock({ file, helperPath }),
      (e) => e.code === "lock_busy",
    );
    await first.release();
    const second = await acquirePrivateLock({ file, helperPath });
    assert.equal(second.identity.inode, first.identity.inode);
    await second.release();
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
});
