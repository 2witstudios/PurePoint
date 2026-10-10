import test from "node:test";
import assert from "node:assert/strict";
import {
  mkdtemp,
  readFile,
  writeFile,
  stat,
  rm,
  symlink,
} from "node:fs/promises";
import path from "node:path";
import os from "node:os";
import { acquirePrivateLock } from "./lock-helper.js";
const helperPath = process.env.POINT_GUARD_LOCK_HELPER_PATH;
test("given owned helper crash should retain kernel exclusion until owner IO drains and releases", async () => {
  const directory = await mkdtemp(path.join(os.tmpdir(), "pointguard-ofd-"));
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
test("given symlink or public inode should refuse without repair", async () => {
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
