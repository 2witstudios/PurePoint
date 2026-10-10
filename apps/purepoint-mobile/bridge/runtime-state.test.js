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
    const inode = (await stat(path.join(dir, "runtime.lock"))).ino;
    await first.close();
    const replacement = await openRuntimeState(dir);
    assert.equal(replacement.value.desktopClientId, identity);
    assert.equal((await stat(path.join(dir, "runtime.lock"))).ino, inode);
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

test("given exact owned runtime crash should reopen permanent inode and restore selection", async () => {
  const { spawn } = await import("node:child_process");
  const { once } = await import("node:events");
  const directory = await mkdtemp(
    path.join(os.tmpdir(), "pointguard-state-crash-"),
  );
  let child;
  try {
    const moduleURL = new URL("./runtime-state.js", import.meta.url).href;
    child = spawn(
      process.execPath,
      [
        "--input-type=module",
        "-e",
        `
      import {openRuntimeState} from ${JSON.stringify(moduleURL)};
      const state = await openRuntimeState(process.argv[1]);
      await state.save({cwd:process.argv[1],provider:"anthropic",model:"fixture-model",selectedSessionPath:"/native/saved.jsonl"});
      console.log(JSON.stringify(state.value));
      setInterval(()=>{},1000);
    `,
        directory,
      ],
      { env: process.env, stdio: ["ignore", "pipe", "pipe"] },
    );
    const selection = await new Promise((resolve, reject) => {
      let output = "";
      const timer = setTimeout(
        () => reject(new Error("Owned crash fixture not ready.")),
        10000,
      );
      child.stdout.on("data", (chunk) => {
        output += chunk;
        if (output.includes("\n")) {
          clearTimeout(timer);
          resolve(JSON.parse(output.trim()));
        }
      });
      child.once("exit", () => {
        clearTimeout(timer);
        reject(new Error("Owned fixture exited before readiness."));
      });
    });
    const inode = (await stat(path.join(directory, "runtime.lock"))).ino;
    const exited = once(child, "exit");
    child.kill("SIGKILL"); // Only this directly spawned disposable fixture.
    await exited;
    let restored;
    const deadline = Date.now() + 10000;
    while (!restored) {
      try {
        restored = await openRuntimeState(directory);
      } catch (error) {
        if (error.code !== "lock_busy" || Date.now() >= deadline) throw error;
        await new Promise((resolve) => setTimeout(resolve, 10));
      }
    }
    try {
      assert.deepEqual(restored.value, selection);
      assert.equal(
        (await stat(path.join(directory, "runtime.lock"))).ino,
        inode,
      );
    } finally {
      await restored.close();
    }
  } finally {
    if (child && child.exitCode === null && child.signalCode === null) {
      const exited = once(child, "exit");
      child.kill("SIGTERM");
      await exited;
    }
    await rm(directory, { recursive: true, force: true });
  }
});

test("unchanged serialized selections preserve the durable file inode", async () => {
  const dir = await mkdtemp(path.join(os.tmpdir(), "pointguard-unchanged-"));
  const state = await openRuntimeState(dir);
  try {
    await state.save({ provider: "anthropic", model: "selected" });
    const before = await stat(path.join(dir, "runtime.json"));
    await Promise.all(Array.from({ length: 25 }, () => state.save({ provider: "anthropic", model: "selected" })));
    const after = await stat(path.join(dir, "runtime.json"));
    assert.equal(after.ino, before.ino);
    assert.equal(after.mtimeMs, before.mtimeMs);
    await state.save({ model: "changed" });
    assert.notEqual((await stat(path.join(dir, "runtime.json"))).ino, before.ino);
  } finally { await state.close(); await rm(dir, { recursive: true, force: true }); }
});
