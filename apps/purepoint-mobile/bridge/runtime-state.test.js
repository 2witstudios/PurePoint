import test from "node:test";
import assert from "node:assert/strict";
import {
  mkdtemp,
  rm,
  readFile,
  writeFile,
  stat,
  symlink,
} from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { openRuntimeState } from "./runtime-state.js";
test("given two owned starts should reject collision and preserve durable selection after replacement", async () => {
  const dir = await mkdtemp(path.join(os.tmpdir(), "pointguard-state-"));
  try {
    const first = await openRuntimeState(dir);
    await first.save({
      selectedSessionPath: "/native/session.jsonl",
      provider: "anthropic",
      model: "model",
      cwd: dir,
    });
    await assert.rejects(openRuntimeState(dir), /already|running|lock/i);
    const identity = first.value.desktopClientId;
    await first.close();
    const replacement = await openRuntimeState(dir);
    assert.equal(replacement.value.desktopClientId, identity);
    assert.equal(
      replacement.value.selectedSessionPath,
      "/native/session.jsonl",
    );
    assert.equal(
      (await stat(path.join(dir, "runtime.json"))).mode & 0o777,
      0o600,
    );
    await replacement.close();
  } finally {
    await rm(dir, { recursive: true, force: true });
  }
});
test("given corrupt or public state should fail closed without replacing contents", async () => {
  const dir = await mkdtemp(path.join(os.tmpdir(), "pointguard-corrupt-"));
  try {
    const file = path.join(dir, "runtime.json");
    const contents = '{"schemaVersion":999}';
    await writeFile(file, contents, { mode: 0o600 });
    await assert.rejects(openRuntimeState(dir));
    assert.equal(await readFile(file, "utf8"), contents);
  } finally {
    await rm(dir, { recursive: true, force: true });
  }
});
test("given symlink state should reject rather than read or overwrite target", async () => {
  const dir = await mkdtemp(path.join(os.tmpdir(), "pointguard-link-"));
  try {
    const target = path.join(dir, "target");
    await writeFile(target, "{}", { mode: 0o600 });
    await symlink(target, path.join(dir, "runtime.json"));
    await assert.rejects(openRuntimeState(dir));
    assert.equal(await readFile(target, "utf8"), "{}");
  } finally {
    await rm(dir, { recursive: true, force: true });
  }
});
