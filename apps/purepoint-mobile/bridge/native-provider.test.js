import test from "node:test";
import assert from "node:assert/strict";
import {
  mkdtemp,
  mkdir,
  chmod,
  writeFile,
  readFile,
  lstat,
  symlink,
  rm,
} from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { nativeProviderSetup } from "./native-provider.js";
import { privateDirectory } from "./runtime-state.js";
import { startupFailure } from "./runtime-errors.js";

async function fixture(t) {
  const home = await mkdtemp(path.join(os.tmpdir(), "native-provider-"));
  const dir = path.join(home, "agent");
  const previous = process.env.PI_CODING_AGENT_DIR;
  process.env.PI_CODING_AGENT_DIR = dir;
  t.after(async () => {
    if (previous === undefined) delete process.env.PI_CODING_AGENT_DIR;
    else process.env.PI_CODING_AGENT_DIR = previous;
    await rm(home, { recursive: true, force: true });
  });
  return { home, dir, auth: path.join(dir, "auth.json") };
}
const bytes =
  '{"anthropic":{"type":"api_key","key":"fixture-not-a-credential"}}\n';
for (const mode of [0o700, 0o750, 0o755]) {
  test(`given a safe native Pi directory (${mode.toString(8)}) should reuse private credentials without changing native data`, async (t) => {
    const { dir, auth } = await fixture(t);
    await mkdir(dir, { mode });
    await chmod(dir, mode);
    await writeFile(auth, bytes, { mode: 0o600 });
    const settings = path.join(dir, "settings.json");
    await writeFile(settings, '{"fixture":"preserve"}\n');
    const provider = await nativeProviderSetup("fixture-device");
    t.after(() => provider.close());
    assert.equal((await lstat(dir)).mode & 0o777, mode);
    assert.equal((await lstat(auth)).mode & 0o777, 0o600);
    assert.equal(await readFile(auth, "utf8"), bytes);
    assert.equal(await readFile(settings, "utf8"), '{"fixture":"preserve"}\n');
  });
}
test("given no native directory should create owner-only setup storage", async (t) => {
  const { dir } = await fixture(t);
  const provider = await nativeProviderSetup("fixture-device");
  t.after(() => provider.close());
  assert.equal((await lstat(dir)).mode & 0o777, 0o700);
});
for (const mode of [0o770, 0o707, 0o777]) {
  test(`given writable native directory (${mode.toString(8)}) should refuse startup without changing credentials or permissions`, async (t) => {
    const { dir, auth } = await fixture(t);
    await mkdir(dir);
    await chmod(dir, mode);
    await writeFile(auth, bytes, { mode: 0o600 });
    await assert.rejects(nativeProviderSetup("fixture-device"), (error) => {
      const failure = startupFailure(error, "credentials", "fixture-instance");
      assert.equal(failure.code, "credential_state");
      assert.match(failure.recovery, /directory/);
      assert.match(failure.recovery, /writ/i);
      assert.equal(JSON.stringify(failure).includes(dir), false);
      return true;
    });
    assert.equal(await readFile(auth, "utf8"), bytes);
    assert.equal((await lstat(dir)).mode & 0o777, mode);
  });
}
for (const kind of ["symlink", "file"]) {
  test(`given a native directory that is a ${kind} should refuse without replacing it`, async (t) => {
    const { home, dir } = await fixture(t);
    const target = path.join(home, "target");
    if (kind === "symlink") {
      await mkdir(target, { mode: 0o700 });
      await symlink(target, dir);
    } else await writeFile(dir, bytes, { mode: 0o600 });
    await assert.rejects(nativeProviderSetup("fixture-device"));
    assert.equal((await lstat(dir)).isSymbolicLink(), kind === "symlink");
    if (kind === "file") assert.equal(await readFile(dir, "utf8"), bytes);
  });
}
for (const [name, mode, content] of [
  ["public", 0o644, bytes],
  ["corrupt", 0o600, "not-json"],
  ["array", 0o600, "[]"],
]) {
  test(`given ${name} native credentials should reject and preserve bytes and permissions`, async (t) => {
    const { dir, auth } = await fixture(t);
    await mkdir(dir, { mode: 0o755 });
    await writeFile(auth, content, { mode });
    await assert.rejects(nativeProviderSetup("fixture-device"), /credentials/);
    assert.equal(await readFile(auth, "utf8"), content);
    assert.equal((await lstat(auth)).mode & 0o777, mode);
  });
}
test("given public managed state should retain strict owner-only validation", async (t) => {
  const { dir } = await fixture(t);
  await mkdir(dir, { mode: 0o755 });
  await assert.rejects(privateDirectory(dir), /private/);
});
test("given a native directory owned by another user should refuse and preserve it", async (t) => {
  const { dir, auth } = await fixture(t);
  await mkdir(dir, { mode: 0o755 });
  await writeFile(auth, bytes, { mode: 0o600 });
  const owner = (await lstat(dir)).uid;
  // Exercise the real directory boundary without requiring privileged chown.
  t.mock.method(process, "getuid", () => owner + 1);
  await assert.rejects(nativeProviderSetup("fixture-device"), /directory/);
  assert.equal((await lstat(dir)).uid, owner);
  assert.equal(await readFile(auth, "utf8"), bytes);
});
test("given symlinked credentials should refuse without changing their target", async (t) => {
  const { dir, home, auth } = await fixture(t);
  await mkdir(dir, { mode: 0o755 });
  const target = path.join(home, "credentials");
  await writeFile(target, bytes, { mode: 0o600 });
  await symlink(target, auth);
  await assert.rejects(nativeProviderSetup("fixture-device"), /credentials/);
  assert.equal((await lstat(auth)).isSymbolicLink(), true);
  assert.equal(await readFile(target, "utf8"), bytes);
});
